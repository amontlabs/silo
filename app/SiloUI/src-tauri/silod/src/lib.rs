#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
compile_error!("silod is an engine for macOS and Linux hosts");

pub mod agent;
pub mod channel;
#[cfg(target_os = "macos")]
pub mod app;
/// The Linux run loop, under the name `entry` dispatches to on both platforms.
#[cfg(target_os = "linux")]
#[path = "linux.rs"]
pub mod app;
#[cfg(target_os = "macos")]
pub mod app_delegate;
#[cfg(target_os = "macos")]
pub mod code_signature;
pub mod context;
pub mod dispatcher;
pub mod framing;
pub mod identity;
#[cfg(target_os = "linux")]
pub mod install_trust;
pub mod manifest;
pub mod methods;
pub mod ownership;
pub mod paths;
pub mod process;
pub mod relay;
#[cfg(target_os = "macos")]
pub mod service;
#[cfg(target_os = "linux")]
#[path = "service_linux.rs"]
pub mod service;
pub mod socket;
pub mod startup;
pub mod timers;

/// Tests that start a child process hold this while they spawn, and tests that need a
/// closed socket hold it from creating the socket until they have used it. macOS has no
/// atomic close-on-exec for new sockets, so a child forked in between keeps the listener
/// open and the socket looks live after it is dropped.
#[cfg(test)]
pub(crate) static FORK_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The operation gate is process-wide while every test engine numbers its jobs from
/// `job:1`, and a job's cancellation depends on whether the gate holds that id. Tests that
/// run jobs hold this for their whole engine's life, so one engine's `job:1` is never
/// mistaken for another's.
#[cfg(test)]
pub(crate) fn serialize_gate_jobs() -> std::sync::MutexGuard<'static, ()> {
    static GATE_JOBS: std::sync::Mutex<()> = std::sync::Mutex::new(());
    GATE_JOBS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The flag the Connections forced SSH command passes (`Channel::remote_bridge_command`).
pub const REMOTE_BRIDGE_FLAG: &str = "--remote-bridge";

pub fn entry(args: &[String]) -> Result<i32, String> {
    match args.get(1).map(String::as_str) {
        Some("--service") => service::run_command(&args[2..]),
        Some("--remote-guest") => app::run_remote_guest(&args[2..]),
        Some(REMOTE_BRIDGE_FLAG) => app::run_remote_bridge(&args[2..]),
        // The runtime command of container computers (`ssh serve`, `exec --tty`), which the editor,
        // SSH access and terminals start where they start `msb` for a virtual machine.
        Some(silo_engine::containers::cli::FLAG) => {
            silo_engine::containers::cli::run(&args[2..])
        }
        Some("--agent") | None => app::run_agent(),
        Some(argument) => Err(format!("unsupported silod argument: {argument}")),
    }
}

/// SSH starts the bridge through the `~/.local/bin` link, and macOS then finds no main bundle
/// (it looks beside the link), so silod cannot read its own identity. The process replaces
/// itself with the real file inside the bundle, keeping its arguments and standard streams.
/// Callers check their arguments first, so a refused call never replaces the process.
pub(crate) fn run_from_bundle(args: &[String]) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    let invoked = std::env::current_exe().map_err(|error| error.to_string())?;
    let canonical = std::fs::canonicalize(&invoked).map_err(|error| error.to_string())?;
    if invoked == canonical {
        return Ok(());
    }
    let error = std::process::Command::new(&canonical)
        .arg0(&canonical)
        .args(&args[1..])
        .exec();
    Err(format!("silod could not start from its bundle: {error}"))
}

#[cfg(test)]
mod entry_tests {
    use silo_engine::channel::Channel;

    /// The forced command a controller's key runs must name a flag this binary handles;
    /// the bridge link points here, so an unknown flag breaks every Connections call.
    #[test]
    fn the_connections_forced_command_names_a_flag_silod_handles() {
        for channel in [Channel::Production, Channel::Development] {
            let command = channel.remote_bridge_command();
            let flag = command.rsplit(' ').next().unwrap();
            assert_eq!(flag, super::REMOTE_BRIDGE_FLAG, "{command}");
        }
    }

    #[test]
    fn the_bridge_takes_only_a_key_fingerprint() {
        let refused = |args: &[&str]| {
            let mut all = vec!["silod", "--remote-bridge"];
            all.extend_from_slice(args);
            let all: Vec<String> = all.into_iter().map(String::from).collect();
            super::entry(&all).unwrap_err()
        };
        for args in [
            &["extra"][..],
            &["--key"],
            &["--key", "SHA256:short"],
            &[
                "--key",
                "SHA256:LH9ifkGkEurhEj7pgiSwRedJtKQomUQeevYOdCTeGWw",
                "extra",
            ],
            &[
                "--other",
                "SHA256:LH9ifkGkEurhEj7pgiSwRedJtKQomUQeevYOdCTeGWw",
            ],
        ] {
            assert_eq!(refused(args), silo_engine::remote::BRIDGE_USAGE, "{args:?}");
        }
        assert!(silo_engine::remote::BRIDGE_USAGE.starts_with("usage: silod --remote-bridge"));
    }
}
