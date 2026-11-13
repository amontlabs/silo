//! Small pieces of the container lifecycle that do not need a Linux host, so they are tested
//! everywhere: the child-reaping guard, the "starting" registry, the holder's cgroup check and the
//! holder log's warnings.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::{
    collections::HashMap,
    process::Child,
    sync::{Mutex, MutexGuard, PoisonError},
    time::Duration,
};

/// A spawned child that is always waited for: if the guard is dropped (an early error return) the
/// child is reaped on a background thread, so it never stays a zombie of the engine.
pub struct Reaped(Option<Child>);

impl Reaped {
    pub fn new(child: Child) -> Self {
        Self(Some(child))
    }

    pub fn child(&mut self) -> &mut Child {
        self.0.as_mut().expect("the child is held until drop")
    }
}

impl Drop for Reaped {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            // Waiting can take as long as the process lives, and the process may be the running
            // container's holder: never make the caller wait for it.
            let _ = std::thread::Builder::new()
                .name("silo-reap".into())
                .spawn(move || {
                    let _ = child.wait();
                });
        }
    }
}

/// The computers this engine process has taken, by name, and whether it is starting them (or
/// reconciling them). A computer being started is neither running nor a leftover: its holder may
/// exist before its gateway does.
static STARTING: Mutex<Option<HashMap<String, bool>>> = Mutex::new(None);

fn starting() -> MutexGuard<'static, Option<HashMap<String, bool>>> {
    STARTING.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Marks a computer as starting until dropped.
pub struct StartMark(String);

impl StartMark {
    /// `None` when the computer is already taken (it is being started, or held).
    pub fn begin(name: &str) -> Option<Self> {
        Self::take(name, true)
    }

    /// Keeps a start from beginning until dropped, without saying the computer is starting: a
    /// checkpoint holds a computer this way, and a running one stays running to everything that
    /// asks (a command, a secret change). `None` when the computer is already taken.
    pub fn hold(name: &str) -> Option<Self> {
        Self::take(name, false)
    }

    fn take(name: &str, starting_it: bool) -> Option<Self> {
        let mut taken = starting();
        let taken = taken.get_or_insert_with(HashMap::new);
        if taken.contains_key(name) {
            return None;
        }
        taken.insert(name.to_owned(), starting_it);
        Some(Self(name.to_owned()))
    }
}

impl Drop for StartMark {
    fn drop(&mut self) {
        if let Some(taken) = starting().as_mut() {
            taken.remove(&self.0);
        }
    }
}

pub fn is_starting(name: &str) -> bool {
    starting()
        .as_ref()
        .is_some_and(|taken| taken.get(name) == Some(&true))
}

/// The command a `crun exec` started in the container, which `silo-init --own-group` made the
/// leader of its own process group. `crun exec --pid-file` records its pid; killing the group
/// ends the command and everything it started, which killing the `crun exec` process does not.
///
/// It owns a private directory holding the pid file and the `--process` JSON (which carries the
/// command's environment values), and removes it when the exec is over.
///
/// The pid file outlives the command (until the exec's owner lets go of it), and by then the number
/// may belong to any other process of the host. So a signal is sent only while the pid is a
/// process of the computer's own scope (`unit`), read from `/proc/<pid>/cgroup` right before.
#[derive(Debug)]
pub struct ExecGroup {
    directory: std::path::PathBuf,
    /// The scope unit of the computer the command runs in (`lifecycle::unit_name`).
    unit: String,
    /// Where the processes are described (`/proc`).
    proc: std::path::PathBuf,
}

const PID_FILE: &str = "exec.pid";
const PROCESS_FILE: &str = "process.json";

impl ExecGroup {
    pub fn new(directory: std::path::PathBuf, unit: &str) -> Self {
        Self {
            directory,
            unit: unit.to_owned(),
            proc: "/proc".into(),
        }
    }

    /// The same group, reading the processes from `proc` instead of `/proc`.
    #[cfg(test)]
    pub fn with_proc(mut self, proc: &std::path::Path) -> Self {
        self.proc = proc.to_owned();
        self
    }

    pub fn pid_file(&self) -> std::path::PathBuf {
        self.directory.join(PID_FILE)
    }

    pub fn process_file(&self) -> std::path::PathBuf {
        self.directory.join(PROCESS_FILE)
    }

    /// SIGKILLs the command's process group. Only a process of the computer's own scope that
    /// leads its own group is touched, so a stale or reused pid cannot take a process of the host
    /// or of another computer with it. Returns whether it signalled.
    pub fn kill(&self) -> bool {
        self.signal(libc::SIGKILL)
    }

    /// Sends `signal` to the command's process group (an SSH client's `signal` request), under the
    /// same rule as [`ExecGroup::kill`]. Returns whether it signalled.
    pub fn signal(&self, signal: i32) -> bool {
        let Some(pid) = leader_pid(&self.directory) else {
            return false;
        };
        if !self.in_computer(pid) {
            return false;
        }
        // SAFETY: getpgid and kill take plain integers.
        unsafe {
            if libc::getpgid(pid) != pid {
                return false;
            }
            libc::kill(-pid, signal) == 0
        }
    }

    /// Whether `pid` is a process of the computer's scope now (`crun exec` puts the command in the
    /// container's control group, below the scope).
    fn in_computer(&self, pid: i32) -> bool {
        std::fs::read_to_string(self.proc.join(pid.to_string()).join("cgroup"))
            .is_ok_and(|text| in_scope(&text, &self.unit))
    }
}

impl Drop for ExecGroup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

fn leader_pid(directory: &std::path::Path) -> Option<i32> {
    std::fs::read_to_string(directory.join(PID_FILE))
        .ok()
        .and_then(|text| text.trim().parse::<i32>().ok())
        .filter(|pid| *pid > 1)
}

/// A new folder for the socket `crun` hands a terminal's controlling side to. Whoever connects to
/// that socket first gives the session its terminal, so only this account may enter the folder,
/// whatever umask the process runs with (a terminal or an editor's proxy starts from a login
/// session, not from the engine).
pub fn console_directory() -> std::io::Result<tempfile::TempDir> {
    use std::os::unix::fs::PermissionsExt;
    tempfile::Builder::new()
        .prefix("silo-pty-")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
}

/// Whether the exec directory `directory` is a leftover: its command is gone (no process has its
/// pid), or it never recorded a pid and has sat unused for `idle`. A command that is alive is
/// never one, whether or not it leads its group yet: `crun exec` records the pid before
/// `silo-init --own-group` has run, and the files of an exec that is starting must stay.
pub fn exec_leftover(directory: &std::path::Path, idle: Duration) -> bool {
    match leader_pid(directory) {
        Some(pid) => !super::image::process_alive(pid as u32),
        None => std::fs::metadata(directory)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > idle),
    }
}

/// Whether the text of `/proc/<pid>/cgroup` places the process in the scope `unit`.scope (or a
/// cgroup below it). A recycled process id is not. The first scope on the path decides: a
/// computer may make cgroups below its own scope, and one named after another computer's scope
/// is still inside its own.
pub fn in_scope(cgroup: &str, unit: &str) -> bool {
    let scope = format!("{unit}.scope");
    cgroup
        .lines()
        .filter_map(|line| line.splitn(3, ':').nth(2))
        .any(|path| path.split('/').find(|part| part.ends_with(".scope")) == Some(scope.as_str()))
}

/// The warnings the holder wrote to its log at or after byte `from` (`silo-ctr: warning: ...`).
pub fn warnings_since(log: &str, from: usize) -> Vec<String> {
    log.get(from..)
        .unwrap_or("")
        .lines()
        .filter_map(|line| line.strip_prefix("silo-ctr: warning: "))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::time::{Duration, Instant};

    fn alive(pid: u32) -> bool {
        // SAFETY: signal 0 only checks that the process exists.
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }

    const UNIT: &str = "silo-ct-ab12cd34-dev";

    /// Describes `pid` in the process folder `proc` as a process of the scope `unit`, the way
    /// `crun exec` leaves a command in the container's control group.
    fn in_computer(proc: &std::path::Path, pid: u32, unit: &str) {
        let dir = proc.join(pid.to_string());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("cgroup"),
            format!(
                "0::/user.slice/user-1000.slice/user@1000.service/app.slice/{unit}.scope/ctr\n"
            ),
        )
        .unwrap();
    }

    #[test]
    fn a_dropped_guard_reaps_its_child_so_no_zombie_is_left() {
        let child = Command::new("sh").args(["-c", "exit 0"]).spawn().unwrap();
        let pid = child.id();
        drop(Reaped::new(child));
        let deadline = Instant::now() + Duration::from_secs(10);
        while alive(pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        // A zombie still answers signal 0; a waited-for process does not.
        assert!(!alive(pid), "the child was never waited for");
    }

    #[test]
    fn killing_the_exec_group_ends_the_command_and_what_it_started() {
        use std::os::unix::process::CommandExt;
        let directory = tempfile::tempdir().unwrap();
        let leaf = directory.path().join("leaf.pid");
        // A leader of its own group (as silo-init --own-group makes it) with a background child.
        let mut leader = Command::new("sh");
        leader
            .args([
                "-c",
                &format!("sleep 300 & echo $! > {}; wait", leaf.display()),
            ])
            .process_group(0);
        let mut leader = leader.spawn().unwrap();
        let pid_file = directory.path().join("exec.pid");
        std::fs::write(&pid_file, format!("{}\n", leader.id())).unwrap();
        std::fs::write(directory.path().join("process.json"), "{}").unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !leaf.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let leaf: u32 = std::fs::read_to_string(&leaf)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let proc = tempfile::tempdir().unwrap();
        in_computer(proc.path(), leader.id(), UNIT);
        let group = ExecGroup::new(directory.path().to_owned(), UNIT).with_proc(proc.path());
        assert_eq!(group.pid_file(), pid_file);
        assert!(group.kill());
        leader.wait().unwrap();
        // The background child is a zombie of init at worst; wait until it is gone or defunct.
        let gone = |pid: u32| {
            std::fs::read_to_string(format!("/proc/{pid}/stat"))
                .map_or(!alive(pid), |stat| stat.contains(") Z"))
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while !gone(leaf) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(gone(leaf), "the command's child survived");
        drop(group);
        assert!(!directory.path().exists(), "the exec's files stayed behind");
    }

    #[test]
    fn a_signal_reaches_the_command_and_what_it_started_and_only_when_it_leads_its_group() {
        use std::os::unix::process::CommandExt;
        let directory = tempfile::tempdir().unwrap();
        let mut leader = Command::new("sh");
        leader
            .args(["-c", "trap 'exit 42' TERM; sleep 300 & wait"])
            .process_group(0);
        let mut leader = leader.spawn().unwrap();
        std::fs::write(
            directory.path().join("exec.pid"),
            format!("{}\n", leader.id()),
        )
        .unwrap();
        let proc = tempfile::tempdir().unwrap();
        in_computer(proc.path(), leader.id(), UNIT);
        in_computer(proc.path(), std::process::id(), UNIT);
        let group = ExecGroup::new(directory.path().to_owned(), UNIT).with_proc(proc.path());
        // Give the shell time to install its trap before the signal arrives.
        std::thread::sleep(Duration::from_millis(300));
        assert!(group.signal(libc::SIGTERM));
        let status = leader.wait().unwrap();
        assert_eq!(
            status.code(),
            Some(42),
            "the command did not get SIGTERM: {status:?}"
        );
        // The command is gone: nothing is signalled any more.
        assert!(!group.signal(libc::SIGTERM));
        // A process that does not lead its group is never signalled.
        std::fs::write(
            directory.path().join("exec.pid"),
            format!("{}\n", std::process::id()),
        )
        .unwrap();
        // SAFETY: getpgrp has no preconditions.
        if unsafe { libc::getpgrp() } as u32 != std::process::id() {
            assert!(!group.signal(libc::SIGTERM));
        }
    }

    #[test]
    fn an_exec_directory_is_a_leftover_only_when_its_command_is_gone() {
        use std::os::unix::process::CommandExt;
        let root = tempfile::tempdir().unwrap();
        let idle = Duration::from_secs(600);
        // A command that leads its own group is running, however old its directory is.
        let mut leader = Command::new("sleep")
            .arg("300")
            .process_group(0)
            .spawn()
            .unwrap();
        let running = root.path().join("running");
        std::fs::create_dir(&running).unwrap();
        std::fs::write(running.join("exec.pid"), format!("{}\n", leader.id())).unwrap();
        assert!(!exec_leftover(&running, idle));
        assert!(!exec_leftover(&running, Duration::ZERO));
        let proc = tempfile::tempdir().unwrap();
        in_computer(proc.path(), leader.id(), UNIT);
        let group = ExecGroup::new(running.clone(), UNIT).with_proc(proc.path());
        assert!(group.kill());
        leader.wait().unwrap();
        assert!(exec_leftover(&running, idle), "the command is gone");
        drop(group);
        // No pid yet: a fresh directory is an exec being set up; an idle one is abandoned.
        let fresh = root.path().join("fresh");
        std::fs::create_dir(&fresh).unwrap();
        assert!(!exec_leftover(&fresh, idle));
        std::thread::sleep(Duration::from_millis(20));
        assert!(exec_leftover(&fresh, Duration::from_millis(1)));
    }

    #[test]
    fn an_exec_whose_command_has_not_taken_its_group_yet_is_not_a_leftover() {
        // `crun exec` records the pid before `silo-init --own-group` makes the command the leader
        // of its group: until then it is alive in the group of what started it, as this one is.
        let mut starting = Command::new("sleep").arg("300").spawn().unwrap();
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("exec.pid"),
            format!("{}\n", starting.id()),
        )
        .unwrap();
        let leftover = exec_leftover(directory.path(), Duration::ZERO);
        starting.kill().unwrap();
        starting.wait().unwrap();
        assert!(
            !leftover,
            "the files of an exec that is starting would be removed"
        );
        assert!(
            exec_leftover(directory.path(), Duration::from_secs(600)),
            "the command is gone"
        );
    }

    #[test]
    fn the_exec_group_never_signals_a_process_that_does_not_lead_its_own_group() {
        let directory = tempfile::tempdir().unwrap();
        let pid_file = directory.path().join("exec.pid");
        // This test process leads no group of its own (its group is the test harness's).
        std::fs::write(&pid_file, format!("{}\n", std::process::id())).unwrap();
        let proc = tempfile::tempdir().unwrap();
        in_computer(proc.path(), std::process::id(), UNIT);
        let group = ExecGroup::new(directory.path().to_owned(), UNIT).with_proc(proc.path());
        // SAFETY: getpgrp has no preconditions.
        if unsafe { libc::getpgrp() } as u32 != std::process::id() {
            assert!(!group.kill());
        }
        for text in ["", "x", "0", "1", "-5"] {
            std::fs::write(&pid_file, text).unwrap();
            assert!(!group.kill(), "{text:?}");
        }
        assert!(!ExecGroup::new(directory.path().join("missing"), UNIT).kill());
    }

    #[test]
    fn a_pid_that_now_belongs_to_a_process_outside_the_computer_is_never_signalled() {
        use std::os::unix::process::CommandExt;
        // The command ended and its pid file stayed (a channel that closes after its command, a
        // terminal that ends); the number now names a process of the host that leads a group of
        // its own, as a shell's job, a daemon or another computer's holder does.
        let mut bystander = Command::new("sleep")
            .arg("300")
            .process_group(0)
            .spawn()
            .unwrap();
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("exec.pid"),
            format!("{}\n", bystander.id()),
        )
        .unwrap();
        let proc = tempfile::tempdir().unwrap();
        // No entry at all, a process of the host, and a process of another computer.
        let group = ExecGroup::new(directory.path().to_owned(), UNIT).with_proc(proc.path());
        assert!(!group.kill());
        let dir = proc.path().join(bystander.id().to_string());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("cgroup"),
            "0::/user.slice/user-1000.slice/session-3.scope\n",
        )
        .unwrap();
        assert!(!group.kill());
        in_computer(proc.path(), bystander.id(), "silo-ct-ab12cd34-other");
        assert!(!group.kill());
        assert!(!group.signal(libc::SIGTERM));
        assert!(
            matches!(bystander.try_wait(), Ok(None)),
            "a process outside the computer was signalled"
        );
        // The same process inside the computer's scope is the command, and is ended.
        in_computer(proc.path(), bystander.id(), UNIT);
        assert!(group.kill());
        bystander.wait().unwrap();
    }

    #[test]
    fn the_folder_of_a_terminals_socket_is_the_accounts_alone() {
        use std::os::unix::fs::PermissionsExt;
        let directory = console_directory().unwrap();
        let mode = std::fs::metadata(directory.path())
            .unwrap()
            .permissions()
            .mode()
            & 0o7777;
        assert_eq!(mode, 0o700, "{mode:o}");
    }

    #[test]
    fn a_guard_gives_access_to_the_child_while_it_lives() {
        let mut guard = Reaped::new(Command::new("sh").args(["-c", "exit 3"]).spawn().unwrap());
        assert_eq!(guard.child().wait().unwrap().code(), Some(3));
    }

    #[test]
    fn a_computer_is_starting_only_between_begin_and_drop_and_only_once() {
        let mark = StartMark::begin("hold-test-a").unwrap();
        assert!(is_starting("hold-test-a"));
        assert!(StartMark::begin("hold-test-a").is_none());
        assert!(!is_starting("hold-test-b"));
        drop(mark);
        assert!(!is_starting("hold-test-a"));
        assert!(StartMark::begin("hold-test-a").is_some());
    }

    #[test]
    fn a_held_computer_cannot_be_started_and_is_not_starting() {
        let hold = StartMark::hold("hold-test-c").unwrap();
        assert!(!is_starting("hold-test-c"));
        assert!(StartMark::begin("hold-test-c").is_none());
        assert!(StartMark::hold("hold-test-c").is_none());
        drop(hold);
        // A start in flight keeps a checkpoint from holding the computer.
        let mark = StartMark::begin("hold-test-c").unwrap();
        assert!(StartMark::hold("hold-test-c").is_none());
        drop(mark);
    }

    #[test]
    fn only_a_process_in_the_computers_own_scope_is_its_holder() {
        let unit = "silo-ct-ab12cd34-dev";
        let holder =
            format!("0::/user.slice/user-1000.slice/user@1000.service/app.slice/{unit}.scope\n");
        assert!(in_scope(&holder, unit));
        assert!(in_scope(&format!("{}/ctr\n", holder.trim_end()), unit));
        assert!(in_scope(&format!("12:cpu:/x\n{holder}"), unit));
        // Another computer, a lookalike name, another cgroup, or nothing.
        assert!(!in_scope(&holder, "silo-ct-ab12cd34-de"));
        assert!(!in_scope(&holder, "silo-ct-ab12cd34-dev2"));
        assert!(!in_scope(
            "0::/user.slice/user-1000.slice/session-3.scope\n",
            unit
        ));
        assert!(!in_scope("", unit));
        // A cgroup another computer made below its own scope, named after this one's.
        let other = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/silo-ct-ab12cd34-other.scope";
        assert!(!in_scope(&format!("{other}/ctr/{unit}.scope\n"), unit));
    }

    #[test]
    fn the_holder_log_yields_only_new_warnings() {
        let old = "silo-ctr: warning: old\nnoise\n";
        let log = format!("{old}silo-ctr: error: x\nsilo-ctr: warning: new one\n");
        assert_eq!(warnings_since(&log, old.len()), ["new one"]);
        assert_eq!(warnings_since(&log, 0), ["old", "new one"]);
        assert!(warnings_since(&log, log.len() + 5).is_empty());
    }
}
