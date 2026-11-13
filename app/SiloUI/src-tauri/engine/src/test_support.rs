//! Fixtures for native unit tests. Never use product locks to serialize tests.
#[path = "test_support/computer_use_live.rs"]
pub(crate) mod computer_use_live;
#[path = "test_support/live.rs"]
pub(crate) mod live;
#[path = "test_support/runner.rs"]
pub(crate) mod runner;

use std::sync::{Mutex, MutexGuard};

static GLOBAL_STATE: Mutex<()> = Mutex::new(());

/// Serializes tests that reach process-wide runtime, GitHub, SSH or secret state.
/// Worker threads belong to their owning test and must be joined before this drops.
pub(crate) fn global_state() -> GlobalStateGuard {
    GlobalStateGuard {
        _guard: GLOBAL_STATE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
    }
}

pub(crate) struct GlobalStateGuard {
    _guard: MutexGuard<'static, ()>,
}
impl Drop for GlobalStateGuard {
    fn drop(&mut self) {
        // Quit tests must reopen admission even when an assertion unwinds.
        crate::runtime::shutdown::cancel();
    }
}

/// The app root for runtime fixtures used by this crate.
pub(crate) fn manifest_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// One disposable runtime layout for in-process runners and fake executable tests.
pub(crate) fn paths(directory: &std::path::Path) -> crate::runtime::RuntimePaths {
    crate::runtime::RuntimePaths {
        guest_image: manifest_root().join("runtime/guest-image"),
        executable: directory.join("msb"),
        home: directory.join("home"),
        storage_home: None,
        library: directory.join("libkrunfw"),
        metadata: directory.join("computers.json"),
        volumes: directory.join("volumes"),
    }
}

/// Install a fake executable only inside a caller-owned temporary directory.
/// Process-boundary tests retain their script bodies; ordinary tests use ScriptedRunner.
pub(crate) fn write_shell_script(path: &std::path::Path, script: impl AsRef<str>) {
    use std::os::unix::fs::PermissionsExt;
    let script = script.as_ref();
    let script = if script.starts_with("#!") {
        script.to_owned()
    } else {
        format!("#!/bin/sh\n{script}")
    };
    std::fs::write(path, script).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}

/// Like `write_shell_script`, then runs the script once with `--first-run`, which it answers by
/// exiting before its own lines. A test whose time limits bound the script uses this, so the
/// limits measure only the script's work (see `run_new_executable_once`).
pub(crate) fn write_run_shell_script(path: &std::path::Path, script: impl AsRef<str>) {
    let script = script.as_ref();
    let (interpreter, body) = match script.strip_prefix("#!") {
        Some(_) => script.split_once('\n').unwrap_or((script, "")),
        None => ("#!/bin/sh", script),
    };
    write_shell_script(
        path,
        format!("{interpreter}\nif [ \"${{1:-}}\" = --first-run ]; then exit 0; fi\n{body}"),
    );
    run_new_executable_once(path, &["--first-run"]);
}

/// Runs a freshly written executable once with `args` and waits for it to finish, however long
/// that takes. On macOS the first run of a new executable file waits for a system check that
/// took seconds while other tests ran (later runs start in milliseconds), so a test that bounds
/// a fake's run with a time limit runs it once first and the limit measures only the fake.
pub(crate) fn run_new_executable_once(path: &std::path::Path, args: &[&str]) {
    let status = std::process::Command::new(path)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "{} {args:?}: {status}", path.display());
}
