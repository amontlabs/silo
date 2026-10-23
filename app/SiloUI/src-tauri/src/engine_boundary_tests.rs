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
fn no_source_names_the_engine_crate() {
    for (path, text) in without_this_file(sources()) {
        // Comments may name the crate; code may not.
        for line in text
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
        {
            assert!(
                !line.contains("silo_engine"),
                "{} names silo_engine; the stateless host helpers live in silo_host",
                path.display()
            );
        }
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
    // The two engine probes under examples/ compile engine modules and have dev-dependencies.
    let manifest = manifest
        .split("# The two engine probes")
        .next()
        .expect("the manifest has runtime dependencies first");
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

fn dependency_tables(table: &toml::Table, found: &mut Vec<(String, toml::Table)>, scope: &str) {
    for (key, value) in table {
        let Some(inner) = value.as_table() else {
            continue;
        };
        match key.as_str() {
            "dependencies" | "dev-dependencies" | "build-dependencies" => {
                found.push((format!("{scope}{key}"), inner.clone()));
            }
            "target" => {
                for (cfg, platform) in inner {
                    if let Some(platform) = platform.as_table() {
                        dependency_tables(platform, found, &format!("target.{cfg}."));
                    }
                }
            }
            _ => {}
        }
    }
}

#[test]
fn the_ui_manifest_has_no_engine_dependency_in_any_table() {
    let text =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    let manifest: toml::Table = text.parse().unwrap();
    let mut tables = Vec::new();
    dependency_tables(&manifest, &mut tables, "");
    assert!(tables.iter().any(|(name, _)| name == "dependencies"));
    for (name, dependencies) in tables {
        for (key, value) in &dependencies {
            let package = value
                .get("package")
                .and_then(toml::Value::as_str)
                .unwrap_or(key);
            assert_ne!(package, "silo-engine", "[{name}] depends on the engine");
        }
    }
}

#[test]
fn the_ui_crate_never_reaches_the_engine_through_any_locked_dependency_path() {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../Cargo.lock"))
        .unwrap();
    let lock: toml::Table = text.parse().unwrap();
    let mut graph = std::collections::BTreeMap::<String, Vec<String>>::new();
    for package in lock["package"].as_array().unwrap() {
        let name = package["name"].as_str().unwrap().to_string();
        let dependencies = package
            .get("dependencies")
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
            .map(|dependency| {
                dependency
                    .as_str()
                    .unwrap()
                    .split(' ')
                    .next()
                    .unwrap()
                    .to_string()
            });
        graph.entry(name).or_default().extend(dependencies);
    }
    assert!(graph.contains_key("silo-ui") && graph.contains_key("silo-host"));
    let mut seen = std::collections::BTreeSet::new();
    let mut pending = vec!["silo-ui".to_string()];
    while let Some(name) = pending.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        pending.extend(graph.get(&name).into_iter().flatten().cloned());
    }
    assert!(
        !seen.contains("silo-engine"),
        "silo-ui reaches silo-engine through its locked dependencies"
    );
    assert!(seen.contains("silo-host"));
}

/// Every method name the app sends is a canonical one: there are no aliases or old names.
#[test]
fn every_forwarded_method_is_in_the_canonical_registry() {
    let registry: std::collections::HashSet<&str> = silo_api::methods::method_registry()
        .iter()
        .map(|descriptor| descriptor.name)
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    for (path, text) in without_this_file(sources()) {
        // Tests use fake method names for the engine's own replies; only code is checked.
        let code = text.split("#[cfg(test)]").next().unwrap_or(&text);
        let mut rest = code;
        while let Some(index) = rest.find(['.', '(']) {
            rest = &rest[index + 1..];
            for call in ["call(", "call_with(", "run_job(", "run_job_until_exit("] {
                if let Some(arguments) = rest.strip_prefix(call) {
                    let arguments = arguments.trim_start();
                    if let Some(quoted) = arguments.strip_prefix('"') {
                        let name = quoted.split('"').next().unwrap_or_default();
                        // Engine methods are `domain.name`; other `call(` users (D-Bus) are not.
                        if !name.contains('.') {
                            continue;
                        }
                        assert!(
                            registry.contains(name),
                            "{} sends {name}, which is not a canonical method",
                            path.display()
                        );
                        seen.insert(name.to_owned());
                    }
                }
            }
        }
    }
    // The scan is not vacuous: the core forwarding paths are among what it saw.
    for expected in [
        "computers.list",
        "operations.job.get",
        "settings.read",
        "engine.shutdown",
    ] {
        assert!(
            seen.contains(expected),
            "{expected} was not found in the sources"
        );
    }
}
