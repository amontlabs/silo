use std::{collections::BTreeSet, sync::Mutex};

use super::*;

/// A stand-in for the engine that records every call and fails the step it is told to.
#[derive(Default)]
struct Fake {
    calls: Mutex<Vec<String>>,
    computers: Mutex<Vec<(String, String)>>,
    /// A call whose text starts with one of these fails.
    fail: Mutex<Vec<String>>,
    /// The fork leaves a half-made computer when it fails.
    half_made: bool,
    logins: Vec<String>,
    /// Paths that survive the removal.
    survivors: Vec<String>,
    deleted: Mutex<BTreeSet<String>>,
    /// What the removal was given on stdin.
    removed: Mutex<Vec<u8>>,
}

impl Fake {
    fn new(logins: &[&str]) -> Self {
        let fake = Fake {
            logins: logins.iter().map(|path| (*path).to_owned()).collect(),
            ..Fake::default()
        };
        fake.computers
            .lock().unwrap()
            .push(("src".into(), "main".into()));
        fake
    }

    fn failing(self, call: &str) -> Self {
        self.fail.lock().unwrap().push(call.to_owned());
        self
    }

    fn step(&self, call: String) -> Result<(), String> {
        self.calls.lock().unwrap().push(call.clone());
        let fails = self
            .fail
            .lock().unwrap()
            .iter()
            .any(|prefix| call.starts_with(prefix.as_str()));
        if fails {
            Err(format!("{call} failed"))
        } else {
            Ok(())
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn called(&self, prefix: &str) -> bool {
        self.calls().iter().any(|call| call.starts_with(prefix))
    }
}

impl ScrubEnv for Fake {
    fn computers(&self) -> Result<Vec<(String, String)>, String> {
        Ok(self.computers.lock().unwrap().clone())
    }

    fn fork(&self, source: &str, name: &str) -> Result<String, String> {
        let result = self.step(format!("fork {source} {name}"));
        if result.is_ok() || self.half_made {
            self.computers
                .lock().unwrap()
                .push(("copy".into(), name.to_owned()));
        }
        result.map(|()| "copy".to_owned())
    }

    fn start(&self, id: &str) -> Result<(), String> {
        self.step(format!("start {id}"))
    }

    fn run(&self, id: &str, script: &str, stdin: &[u8]) -> Result<ExecOut, String> {
        let kind = if script == REMOVE_SCRIPT {
            "remove"
        } else if script == SURVIVORS_SCRIPT {
            "survivors"
        } else {
            panic!("an unexpected command: {script}")
        };
        self.step(format!("run {id} {kind}"))?;
        if kind == "remove" {
            *self.removed.lock().unwrap() = stdin.to_vec();
        }
        let mut out = ExecOut::default();
        if kind == "survivors" {
            assert!(!stdin.is_empty());
            out.stdout = self
                .survivors
                .iter()
                .map(|path| format!("{path}\n"))
                .collect::<String>()
                .into_bytes();
        }
        Ok(out)
    }

    fn login_paths(&self, id: &str) -> Result<Vec<String>, String> {
        self.step(format!("logins {id}"))?;
        Ok(self.logins.clone())
    }

    fn stop(&self, id: &str) -> Result<(), String> {
        self.step(format!("stop {id}"))
    }

    fn delete(&self, id: &str) -> Result<(), String> {
        let result = self.step(format!("delete {id}"));
        if result.is_ok() {
            self.computers.lock().unwrap().retain(|(known, _)| known != id);
            self.deleted.lock().unwrap().insert(id.to_owned());
        }
        result
    }
}

fn progress_into(lines: &Mutex<Vec<String>>) -> impl Fn(&str) + '_ {
    |line| lines.lock().unwrap().push(line.to_owned())
}

const LOGINS: [&str; 2] = [
    "/root/.claude/.credentials.json",
    "/home/silo/.claude.json",
];

#[test]
fn a_copy_is_named_so_that_only_silo_can_make_one() {
    let name = copy_name();
    assert!(name.starts_with("silo-export-"));
    assert!(is_copy(&name));
    assert_ne!(name, copy_name());
    for other in [
        "silo-export-",
        "silo-export-xyz12345",
        "silo-export-0123456",
        "silo-export-0123456789",
        "main",
        "silo-build-dev-0123abcd",
        "my-silo-export-0123abcd",
    ] {
        assert!(!is_copy(other), "{other}");
    }
    assert!(crate::computer_tree::hidden(&name));
}

#[test]
fn a_copy_without_logins_is_made_started_stripped_and_stopped() {
    let env = Fake::new(&LOGINS);
    let lines = Mutex::new(Vec::new());
    let prepared = prepare(&env, "src", &progress_into(&lines), &|| false).unwrap();
    assert_eq!(prepared.copy_id, "copy");
    assert!(is_copy(&prepared.copy_name));
    assert_eq!(
        prepared.left_out,
        [
            "Claude Code sign-in data (/home/silo/.claude.json)",
            "a Claude Code login (/root/.claude/.credentials.json)"
        ]
    );
    let calls = env.calls();
    let order = [
        "fork src",
        "start copy",
        "logins copy",
        "run copy remove",
        "run copy survivors",
        "stop copy",
    ];
    let mut at = 0;
    for expected in order {
        at += calls[at..]
            .iter()
            .position(|call| call.starts_with(expected))
            .unwrap_or_else(|| panic!("{expected} missing or out of order in {calls:?}"));
    }
    // Nothing is dropped yet: the export reads the copy next.
    assert!(!env.called("delete"));
    assert_eq!(lines.lock().unwrap().len(), 5);
    assert!(lines.lock().unwrap()[0].starts_with("Making a temporary copy"));
}

#[test]
fn the_copy_always_loses_the_endpoint_file_of_silos_tools() {
    for logins in [&[][..], &LOGINS[..]] {
        let env = Fake::new(logins);
        let prepared = prepare(&env, "src", &|_| {}, &|| false).unwrap();
        assert_eq!(prepared.left_out.is_empty(), logins.is_empty());
        assert!(env.called("run copy remove"));
        let removed = String::from_utf8(env.removed.lock().unwrap().clone()).unwrap();
        let removed: Vec<&str> = removed.split('\0').filter(|path| !path.is_empty()).collect();
        assert!(removed.contains(&"/etc/silo/endpoint.json"), "{removed:?}");
        assert!(removed.contains(&"/etc/silo/guest-client.json"));
        for login in logins {
            assert!(removed.contains(login));
        }
    }
}

#[test]
fn the_copy_is_dropped_whatever_step_fails() {
    for failing in ["start", "logins", "run copy remove", "run copy survivors", "stop"] {
        let env = Fake::new(&LOGINS).failing(failing);
        let error = prepare(&env, "src", &|_| {}, &|| false).unwrap_err();
        let Failure::Failed(message) = error else {
            panic!("{failing}: not a failure")
        };
        assert!(message.contains("failed") || message.contains("Silo could not"), "{message}");
        assert!(env.deleted.lock().unwrap().contains("copy"), "{failing}: the copy stays");
        assert!(
            env.computers.lock().unwrap().iter().all(|(_, name)| !is_copy(name)),
            "{failing}"
        );
    }
}

#[test]
fn a_fork_that_fails_leaves_no_copy_and_no_checkpoint() {
    for half_made in [false, true] {
        let mut env = Fake::new(&LOGINS).failing("fork");
        env.half_made = half_made;
        let error = prepare(&env, "src", &|_| {}, &|| false).unwrap_err();
        assert!(matches!(error, Failure::Failed(_)));
        assert_eq!(env.called("delete copy"), half_made);
    }
}

#[test]
fn files_that_survive_the_removal_fail_the_export_and_are_named() {
    let mut env = Fake::new(&LOGINS);
    env.survivors = vec!["/root/.claude/.credentials.json".into()];
    let Failure::Failed(message) = prepare(&env, "src", &|_| {}, &|| false).unwrap_err() else {
        panic!("expected a failure")
    };
    assert!(message.contains("/root/.claude/.credentials.json"), "{message}");
    assert!(!env.called("stop copy"));
    assert!(env.deleted.lock().unwrap().contains("copy"));
}

#[test]
fn a_cleanup_that_fails_is_reported_with_the_failure() {
    let env = Fake::new(&LOGINS).failing("start").failing("delete copy");
    let Failure::Failed(message) = prepare(&env, "src", &|_| {}, &|| false).unwrap_err() else {
        panic!("expected a failure")
    };
    assert!(message.contains("start copy failed"), "{message}");
    assert!(message.contains("could not drop the temporary copy"), "{message}");
}

#[test]
fn a_cancel_stops_the_work_and_drops_the_copy() {
    // Before anything: nothing is made.
    let env = Fake::new(&LOGINS);
    assert_eq!(
        prepare(&env, "src", &|_| {}, &|| true).unwrap_err(),
        Failure::Cancelled
    );
    assert!(!env.called("fork"));
    // After the copy exists: it goes.
    let env = Fake::new(&LOGINS);
    let asked = Mutex::new(0);
    let cancelled = || {
        *asked.lock().unwrap() += 1;
        *asked.lock().unwrap() > 2
    };
    assert_eq!(
        prepare(&env, "src", &|_| {}, &cancelled).unwrap_err(),
        Failure::Cancelled
    );
    assert!(env.deleted.lock().unwrap().contains("copy"));
}

#[test]
fn a_second_computers_copy_leaves_the_first_ones_alone() {
    let env = Fake::new(&LOGINS);
    let first = prepare(&env, "src", &|_| {}, &|| false).unwrap();
    let second = prepare(&env, "src", &|_| {}, &|| false).unwrap();
    assert_eq!(first.copy_id, second.copy_id);
    assert!(!env.deleted.lock().unwrap().contains("copy"));
}

#[test]
fn copies_an_earlier_run_left_behind_are_dropped_first() {
    let env = Fake::new(&LOGINS);
    env.computers
        .lock().unwrap()
        .push(("stale".into(), "silo-export-0123abcd".into()));
    env.computers
        .lock().unwrap()
        .push(("other".into(), "silo-build-dev-0123abcd".into()));
    drop_stale(&env);
    prepare(&env, "src", &|_| {}, &|| false).unwrap();
    assert!(env.deleted.lock().unwrap().contains("stale"));
    assert!(!env.deleted.lock().unwrap().contains("other"));
    assert!(!env.deleted.lock().unwrap().contains("src"));
}

#[test]
fn dropping_a_prepared_copy_removes_it_and_a_copy_that_will_not_go_is_reported() {
    let env = Fake::new(&LOGINS);
    let prepared = prepare(&env, "src", &|_| {}, &|| false).unwrap();
    env.fail.lock().unwrap().push("delete copy".into());
    let error = drop_copy(&env, &prepared).unwrap_err();
    assert!(error.contains("could not drop the temporary copy"), "{error}");
    env.fail.lock().unwrap().clear();
    drop_copy(&env, &prepared).unwrap();
    assert!(env.deleted.lock().unwrap().contains("copy"));
}

#[test]
fn paths_reach_the_guest_on_stdin_so_no_name_is_taken_for_an_option_or_a_command() {
    assert_eq!(
        nul_separated(&["/a b".into(), "/c\nd".into()]),
        b"/a b\0/c\nd\0".to_vec()
    );
    let env = Fake::new(&[]);
    assert!(remove(&env, "copy", &["relative".into()]).is_err());
    assert!(!env.called("run"));
}
