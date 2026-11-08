//! Runs the built `silo-init` against the real kernel. Each test skips (and says why) when Landlock or the
//! unprivileged namespaces it needs are unavailable.
#![cfg(target_os = "linux")]

use std::path::PathBuf;
use std::process::{Command, Output};

fn init() -> Command {
    Command::new(env!("CARGO_BIN_EXE_silo-init"))
}

fn landlock_available() -> bool {
    let output = init()
        .arg("--probe")
        .output()
        .expect("run silo-init --probe");
    String::from_utf8_lossy(&output.stdout).starts_with("landlock abi")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("silo-init-test-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create scratch directory");
    dir
}

fn text(output: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn probe_reports_the_abi_or_unavailable() {
    let output = init().arg("--probe").output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    let (out, _) = text(&output);
    assert!(
        out.starts_with("landlock abi ") || out == "landlock unavailable\n",
        "{out:?}"
    );
}

#[test]
fn a_missing_command_is_a_usage_error_and_a_missing_program_exits_127() {
    assert_eq!(init().output().unwrap().status.code(), Some(2));
    let output = init()
        .args(["--", "/nonexistent/program"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(127));
    assert!(text(&output)
        .1
        .contains("exec /nonexistent/program: No such file or directory"));
}

#[test]
fn the_exit_status_passes_through() {
    let output = init().args(["--", "sh", "-c", "exit 7"]).output().unwrap();
    assert_eq!(output.status.code(), Some(7));
}

#[test]
fn writes_are_allowed_under_the_rw_list_and_denied_elsewhere() {
    if !landlock_available() {
        eprintln!("skipped: Landlock is unavailable on this kernel");
        return;
    }
    let (allowed, denied) = (scratch("allowed"), scratch("denied"));
    let write = |dir: &PathBuf, extra: &[String]| {
        init()
            .args(extra)
            .args([
                "--",
                "sh",
                "-c",
                &format!("echo data > {}/file", dir.display()),
            ])
            .output()
            .unwrap()
    };
    let rw = vec![format!("--rw={}", allowed.display())];
    assert!(write(&allowed, &rw).status.success());
    assert_eq!(
        std::fs::read_to_string(allowed.join("file")).unwrap(),
        "data\n"
    );
    let blocked = write(&denied, &rw);
    assert!(!blocked.status.success());
    assert!(
        text(&blocked).1.contains("Permission denied"),
        "{:?}",
        text(&blocked)
    );
    assert!(!denied.join("file").exists());
    // The default rw list covers /tmp, where both directories live.
    assert!(write(&denied, &[]).status.success());
    assert!(denied.join("file").exists());
    let _ = std::fs::remove_dir_all(&allowed);
    let _ = std::fs::remove_dir_all(&denied);
}

#[test]
fn missing_paths_in_the_lists_are_skipped() {
    if !landlock_available() {
        eprintln!("skipped: Landlock is unavailable on this kernel");
        return;
    }
    let output = init()
        .args(["--ro=/", "--rw=/nonexistent-silo-path:/tmp", "--", "true"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", text(&output));
}

/// Runs `silo-init` as PID 1 of a fresh PID namespace, or None (with the reason printed) when the host
/// does not allow it.
fn as_pid_one(script: &str) -> Option<Output> {
    let unshare = |command: &[&str]| {
        Command::new("unshare")
            .args([
                "--user",
                "--map-root-user",
                "--pid",
                "--fork",
                "--mount-proc",
            ])
            .args(command)
            .output()
    };
    match unshare(&["true"]) {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            eprintln!(
                "skipped: unshare is not permitted here: {}",
                text(&output).1.trim()
            );
            return None;
        }
        Err(error) => {
            eprintln!("skipped: unshare is not available: {error}");
            return None;
        }
    }
    let init_path = env!("CARGO_BIN_EXE_silo-init");
    Some(unshare(&[init_path, "--", "sh", "-c", script]).expect("run unshare"))
}

#[test]
fn as_pid_one_it_reaps_orphans_and_passes_the_status_through() {
    if !landlock_available() {
        eprintln!("skipped: Landlock is unavailable on this kernel");
        return;
    }
    // The grandchild is orphaned at once; PID 1 must collect it. /proc/1/comm shows who PID 1 is.
    let script = "(true &); sleep 0.5; \
                  echo init=$(cat /proc/1/comm); \
                  echo zombies=$(grep -l '^State:.Z' /proc/[0-9]*/status 2>/dev/null | wc -l); exit 9";
    let Some(output) = as_pid_one(script) else {
        return;
    };
    let (out, err) = text(&output);
    assert!(out.contains("init=silo-init"), "{out:?} {err:?}");
    assert!(out.contains("zombies=0"), "{out:?} {err:?}");
    assert_eq!(output.status.code(), Some(9), "{err:?}");
}

#[test]
fn as_pid_one_a_signalled_child_gives_128_plus_the_signal() {
    if !landlock_available() {
        eprintln!("skipped: Landlock is unavailable on this kernel");
        return;
    }
    let Some(output) = as_pid_one("kill -KILL $$") else {
        return;
    };
    assert_eq!(output.status.code(), Some(128 + 9), "{:?}", text(&output));
}
