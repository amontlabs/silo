//! Small pieces of the container lifecycle that do not need a Linux host, so they are tested
//! everywhere: the child-reaping guard, the "starting" registry, the holder's cgroup check and the
//! holder log's warnings.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::{
    collections::HashSet,
    process::Child,
    sync::{Mutex, MutexGuard, PoisonError},
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

/// The computers this engine process is starting (or reconciling) right now, by name. A computer
/// in it is neither running nor a leftover: its holder may exist before its gateway does.
static STARTING: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn starting() -> MutexGuard<'static, Option<HashSet<String>>> {
    STARTING.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Marks a computer as starting until dropped.
pub struct StartMark(String);

impl StartMark {
    /// `None` when the computer is already being started.
    pub fn begin(name: &str) -> Option<Self> {
        starting()
            .get_or_insert_with(HashSet::new)
            .insert(name.to_owned())
            .then(|| Self(name.to_owned()))
    }
}

impl Drop for StartMark {
    fn drop(&mut self) {
        if let Some(set) = starting().as_mut() {
            set.remove(&self.0);
        }
    }
}

pub fn is_starting(name: &str) -> bool {
    starting().as_ref().is_some_and(|set| set.contains(name))
}

/// The command a `crun exec` started in the container, which `silo-init --own-group` made the
/// leader of its own process group. `crun exec --pid-file` records its pid; killing the group
/// ends the command and everything it started, which killing the `crun exec` process does not.
pub struct ExecGroup {
    pid_file: std::path::PathBuf,
}

impl ExecGroup {
    pub fn new(pid_file: std::path::PathBuf) -> Self {
        Self { pid_file }
    }

    pub fn pid_file(&self) -> &std::path::Path {
        &self.pid_file
    }

    /// SIGKILLs the command's process group. Only a process that leads its own group is touched,
    /// so a stale or reused pid cannot take anything else with it. Returns whether it signalled.
    pub fn kill(&self) -> bool {
        let Some(pid) = std::fs::read_to_string(&self.pid_file)
            .ok()
            .and_then(|text| text.trim().parse::<i32>().ok())
            .filter(|pid| *pid > 1)
        else {
            return false;
        };
        // SAFETY: getpgid and kill take plain integers.
        unsafe {
            if libc::getpgid(pid) != pid {
                return false;
            }
            libc::kill(-pid, libc::SIGKILL) == 0
        }
    }
}

impl Drop for ExecGroup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.pid_file);
    }
}

/// Whether the text of `/proc/<pid>/cgroup` places the process in the scope `unit`.scope (or a
/// cgroup below it). A recycled process id is not.
pub fn in_scope(cgroup: &str, unit: &str) -> bool {
    let scope = format!("{unit}.scope");
    cgroup
        .lines()
        .filter_map(|line| line.splitn(3, ':').nth(2))
        .any(|path| path.split('/').any(|part| part == scope))
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
        let deadline = Instant::now() + Duration::from_secs(10);
        while !leaf.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let leaf: u32 = std::fs::read_to_string(&leaf)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let group = ExecGroup::new(pid_file.clone());
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
        assert!(!pid_file.exists());
    }

    #[test]
    fn the_exec_group_never_signals_a_process_that_does_not_lead_its_own_group() {
        let directory = tempfile::tempdir().unwrap();
        let pid_file = directory.path().join("exec.pid");
        // This test process leads no group of its own (its group is the test harness's).
        std::fs::write(&pid_file, format!("{}\n", std::process::id())).unwrap();
        let group = ExecGroup::new(pid_file.clone());
        // SAFETY: getpgrp has no preconditions.
        if unsafe { libc::getpgrp() } as u32 != std::process::id() {
            assert!(!group.kill());
        }
        for text in ["", "x", "0", "1", "-5"] {
            std::fs::write(&pid_file, text).unwrap();
            assert!(!group.kill(), "{text:?}");
        }
        assert!(!ExecGroup::new(directory.path().join("missing")).kill());
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
