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
    prepare, scripts, Hooks, Outcome, Request, Resumed, UndoOutcome,
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
    let plan = plan::build(&changes, &base, &fork, &base, Os::Linux);
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
    let plan = plan::build(&changes, &base, &fork, &base, Os::Linux);
    assert!(plan.packages.is_empty());
    assert_eq!(plan.conflicts[0].kind, "package");
}

#[test]
fn a_package_main_changed_since_the_fork_is_a_conflict_and_a_removal_is_three_way() {
    let base = m("PKG\tdpkg\tjq\t1.6\nPKG\tdpkg\told\t1\nPKGM\tdpkg\tjq\nPKGM\tdpkg\told\n");
    let fork = m("PKG\tdpkg\tjq\t1.7\nPKGM\tdpkg\tjq\n");
    let main = m("PKG\tdpkg\tjq\t1.5\nPKG\tdpkg\told\t1\nPKGM\tdpkg\tjq\nPKGM\tdpkg\told\n");
    let changes = diff(&base, &fork, None, &IgnoreList::builtin());
    let plan = plan::build(&changes, &base, &fork, &main, Os::Linux);
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
    let plan = plan::build(&changes, &base, &fork, &main_edited, Os::Linux);
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
    assert_eq!(refusal.reason, "divergent");
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
    assert_eq!(refusal.reason, "divergent");
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
    assert_eq!(refusal.reason, "git");
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
    assert!(refusal.hint.contains("Make this main"));
    assert!(refusal.hint.contains("--promote"), "{}", refusal.hint);
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
        .any(|l| l.contains("will keep packages: jq 1.7")));
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
    st.fork_later = Some(mt(
        "F\t/root/.bashrc\t12\t2\tbb\nF\t/root/new.txt\t1\t3\tcc\n",
    ));
    let refusal = refused(run(&world, &st, &journal, &request(Hooks::default())).unwrap());
    assert_eq!(refusal.reason, "changed");
    assert_eq!(world.logged("file_write"), 0);
    assert_eq!(journal.latest_undoable("main"), None);
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
    assert!(refusal.message.contains("package version: brew jq"));
    assert!(!refusal.message.contains("clash"));
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
    st.main_later = Some(mt(&format!("{BASE}F\t/root/new.txt\t10\t3\tc:x\n")));
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
