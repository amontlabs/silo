#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

#[cfg(not(target_os = "macos"))]
compile_error!("silod is a macOS-host engine process");

pub mod app;
pub mod app_delegate;
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
pub mod startup;
pub mod timers;

pub fn entry(args: &[String]) -> Result<i32, String> {
    match args.get(1).map(String::as_str) {
        Some("--service") => service::run_command(&args[2..]),
        Some("--remote-guest") => app::run_remote_guest(&args[2..]),
        Some("--agent") | None => app::run_agent(),
        Some(argument) => Err(format!("unsupported silod argument: {argument}")),
    }
}
