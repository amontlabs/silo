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
