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

/// The string literals of Rust source, without comments. Raw strings and byte strings are
/// read like ordinary ones; a quote character literal is skipped.
fn string_literals(source: &str) -> Vec<String> {
    let characters: Vec<char> = source.chars().collect();
    let mut literals = Vec::new();
    let mut index = 0;
    while index < characters.len() {
        match characters[index] {
            '/' if characters.get(index + 1) == Some(&'/') => {
                while index < characters.len() && characters[index] != '\n' {
                    index += 1;
                }
            }
            '/' if characters.get(index + 1) == Some(&'*') => {
                let mut depth = 0;
                while index < characters.len() {
                    if characters[index] == '/' && characters.get(index + 1) == Some(&'*') {
                        depth += 1;
                        index += 2;
                    } else if characters[index] == '*' && characters.get(index + 1) == Some(&'/') {
                        depth -= 1;
                        index += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        index += 1;
                    }
                }
            }
            '\'' if characters.get(index + 1) == Some(&'"')
                && characters.get(index + 2) == Some(&'\'') =>
            {
                index += 3;
            }
            '"' => {
                let mut literal = String::new();
                index += 1;
                while index < characters.len() && characters[index] != '"' {
                    if characters[index] == '\\' {
                        literal.push(characters[index]);
                        index += 1;
                    }
                    if let Some(character) = characters.get(index) {
                        literal.push(*character);
                    }
                    index += 1;
                }
                index += 1;
                literals.push(literal);
            }
            _ => index += 1,
        }
    }
    literals
}

/// The app never names an engine method: every call goes through the typed constructors of
/// `silo_api::methods::requests`, so a wrong name or parameter cannot compile. No string
/// literal in the app equals a method name, and none is a template that builds one.
#[test]
fn no_source_names_an_engine_method() {
    let methods: Vec<&str> = silo_api::methods::method_registry()
        .iter()
        .map(|descriptor| descriptor.name)
        .collect();
    let domains: std::collections::BTreeSet<String> = methods
        .iter()
        .filter_map(|name| name.split('.').next())
        .map(|domain| format!("{domain}."))
        .collect();
    let files = sources();
    assert!(files.len() > 40, "the scan saw only {} files", files.len());
    let mut typed_requests = 0;
    for (path, text) in &files {
        typed_requests += text.matches("requests::").count();
        for literal in string_literals(text) {
            assert!(
                !methods.contains(&literal.as_str()),
                "{} names the engine method {literal}; build a request with `requests::`",
                path.display()
            );
            // A `format!` template that builds a method name, such as `<domain>.{action}`.
            assert!(
                !(literal.contains('{')
                    && domains.iter().any(|domain| literal.starts_with(domain))),
                "{} builds an engine method name from {literal}",
                path.display()
            );
        }
    }
    // The scan is not vacuous: the app's calls are typed requests.
    assert!(
        typed_requests > 100,
        "only {typed_requests} typed requests were found"
    );
}

#[test]
fn the_literal_scan_reads_strings_but_not_comments_or_quote_characters() {
    let source = "let a = \"x.y\"; // \"no\"\n/* \"no\" */ let b = '\"'; let c = \"p\\\"q\";";
    assert_eq!(string_literals(source), ["x.y", "p\\\"q"]);
}
