//! Silo.app is a client of `silod`: it never runs the engine in its own process (95 A-1).
//! These checks fail if engine construction, in-process engine operations or engine timers
//! come back into the Tauri crate.

use std::path::{Path, PathBuf};

fn sources() -> Vec<(PathBuf, String)> {
    fn walk(directory: &Path, found: &mut Vec<(PathBuf, String)>) {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, found);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                found.push((path, text));
            }
        }
    }
    let mut found = Vec::new();
    walk(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut found,
    );
    found
}

fn without_this_file(files: Vec<(PathBuf, String)>) -> Vec<(PathBuf, String)> {
    files
        .into_iter()
        .filter(|(path, _)| !path.ends_with("engine_boundary_tests.rs"))
        .collect()
}

#[test]
fn no_source_constructs_or_calls_the_engine_in_process() {
    let forbidden = [
        "engine_context",
        "context::install",
        "context::current()",
        "EngineRuntimeState",
        "runtime::OPERATIONS",
        "macos_computers_engine",
        "silo_engine::runtime",
        "silo_engine::settings",
        "silo_engine::jobs",
        "silo_engine::startup",
        "silo_engine::transfer",
        "crate::runtime",
        "crate::settings",
        "crate::startup",
        "stop_local_computers",
        "stop_all(",
        "exit_backstop",
        "remote_network::close_all",
        "ssh_access::close_all",
    ];
    for (path, text) in without_this_file(sources()) {
        for pattern in forbidden {
            assert!(
                !text.contains(pattern),
                "{} reaches the engine in-process through `{pattern}`",
                path.display()
            );
        }
    }
}

#[test]
fn only_the_stateless_host_helpers_are_borrowed_from_the_engine_crate() {
    let allowed = [
        "applications",
        "notifications",
        "system_integrations",
        "system_shutdown",
        "terminal",
    ];
    for (path, text) in without_this_file(sources()) {
        for line in text.lines().filter(|line| line.contains("silo_engine")) {
            // Comments may name the crate; code may only name the helper modules.
            if line.trim_start().starts_with("//") {
                continue;
            }
            assert!(
                path.ends_with("main.rs"),
                "{} names silo_engine; only main.rs re-exports the host helpers",
                path.display()
            );
            let _ = line;
        }
    }
    let main =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs")).unwrap();
    let reexport = main
        .split("pub(crate) use silo_engine::{")
        .nth(1)
        .and_then(|rest| rest.split("};").next())
        .expect("main.rs re-exports the host helpers");
    for module in reexport
        .split(',')
        .map(str::trim)
        .filter(|module| !module.is_empty())
    {
        assert!(
            allowed.contains(&module),
            "{module} is not a stateless host helper"
        );
    }
}

#[test]
fn setup_builds_a_client_and_the_exit_path_never_stops_computers_by_itself() {
    let main =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs")).unwrap();
    assert!(main.contains("engine_client::install(app.handle())"));
    assert!(main.contains("prevent_exit_until_saved"));
    // Quitting stops computers only through the user's choice in the Quit controller.
    let quit =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/quit.rs")).unwrap();
    assert!(quit.contains("StopAndQuit"));
}

#[test]
fn the_ui_crate_does_not_depend_on_the_engine_runtime_crates() {
    let manifest =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    for dependency in [
        "microsandbox-image",
        "microsandbox-utils",
        "keyring",
        "reqwest",
        "objc2-virtualization",
    ] {
        assert!(
            !manifest.contains(&format!("{dependency} =")),
            "{dependency} belongs to the engine, not to the client"
        );
    }
    assert!(manifest.contains("silo-client"));
}
