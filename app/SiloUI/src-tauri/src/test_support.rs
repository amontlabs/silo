//! Fixtures for native unit tests. Never use product locks to serialize tests.
#[cfg(feature = "engine-sources")]
#[path = "test_support/computer_use_live.rs"]
pub(crate) mod computer_use_live;
#[cfg(not(feature = "engine-sources"))]
pub(crate) mod computer_use_live;
#[cfg(feature = "engine-sources")]
#[path = "test_support/live.rs"]
pub(crate) mod live;
#[cfg(not(feature = "engine-sources"))]
pub(crate) mod live;
#[cfg(feature = "engine-sources")]
#[path = "test_support/runner.rs"]
pub(crate) mod runner;
#[cfg(not(feature = "engine-sources"))]
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

/// The package root while these fixtures compile from either SiloUI or silo-engine.
pub(crate) fn manifest_root() -> std::path::PathBuf {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    #[cfg(feature = "engine-sources")]
    let root = root.join("..");
    root
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
