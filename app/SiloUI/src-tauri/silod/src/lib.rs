#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

#[cfg(not(target_os = "macos"))]
compile_error!("silod is a macOS-host engine process");

pub mod app;
pub mod code_signature;
pub mod context;
pub mod dispatcher;
pub mod framing;
pub mod identity;
pub mod manifest;
pub mod methods;
pub mod ownership;
pub mod paths;
pub mod service;
pub mod socket;

/// Tests that start a child process hold this while they spawn, and tests that need a
/// closed socket hold it from creating the socket until they have used it. macOS has no
/// atomic close-on-exec for new sockets, so a child forked in between keeps the listener
/// open and the socket looks live after it is dropped.
#[cfg(test)]
pub(crate) static FORK_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn entry(args: &[String]) -> Result<i32, String> {
    match args.get(1).map(String::as_str) {
        Some("--service") => service::run_command(&args[2..]),
        Some("--remote-guest") => app::run_remote_guest(&args[2..]),
        Some("--agent") | None => app::run_agent(),
        Some(argument) => Err(format!("unsupported silod argument: {argument}")),
    }
}
