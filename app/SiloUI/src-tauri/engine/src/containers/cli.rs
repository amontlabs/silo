//! The container computer's runtime command: what `command_for` runs where a virtual machine's
//! callers run `msb`. The executable is `silod` with [`FLAG`] first, the rest being the
//! MicroSandbox argument shape the caller already builds (`ssh serve NAME ...`, `exec NAME ... --tty`).
//! The runtime home and tools come from the same environment the callers set for `msb`.

use super::args::{self, Command};
use crate::runtime::RuntimePaths;
use std::path::PathBuf;

/// The first argument of the container runtime command.
pub const FLAG: &str = "--container-runtime";

/// The runtime paths a caller passes in the environment it also sets for `msb`.
pub fn paths_from_env() -> Result<RuntimePaths, String> {
    let var = |name: &str| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .ok_or_else(|| format!("{name} is not set."))
    };
    Ok(RuntimePaths {
        guest_image: PathBuf::new(),
        executable: var("MSB_PATH")?,
        home: var("MSB_HOME")?,
        storage_home: None,
        library: var("MSB_LIBKRUNFW_PATH").unwrap_or_default(),
        metadata: PathBuf::new(),
        volumes: PathBuf::new(),
    })
}

/// Runs the command (the arguments after [`FLAG`]) and returns the process exit code.
pub fn run(args: &[String]) -> Result<i32, String> {
    run_with(&paths_from_env()?, args)
}

pub fn run_with(paths: &RuntimePaths, arguments: &[String]) -> Result<i32, String> {
    match args::parse(arguments)? {
        Command::SshServe(serve) => super::ssh::serve(paths, &serve),
        Command::Exec(exec) if exec.tty => terminal(paths, &exec),
        _ => Err("This command is not available for container computers.".into()),
    }
}

/// A terminal in the computer, on the terminal this process is attached to.
#[cfg(target_os = "linux")]
fn terminal(paths: &RuntimePaths, exec: &args::Exec) -> Result<i32, String> {
    use super::{lifecycle, tools::Tools};
    let tools = Tools::locate(paths).map_err(|error| error.to_string())?;
    if !lifecycle::holder_alive(paths, &exec.name) {
        return Err(format!("Start {} first.", exec.name));
    }
    let lifecycle::ExecPlan {
        mut command,
        group,
    } = lifecycle::exec_command(paths, &tools, &exec.name, exec, false)?;
    let status = command
        .status()
        .map_err(|error| format!("The terminal could not start: {error}"))?;
    group.kill();
    use std::os::unix::process::ExitStatusExt;
    Ok(status
        .code()
        .or_else(|| status.signal().map(|signal| 128 + signal))
        .unwrap_or(1))
}

#[cfg(not(target_os = "linux"))]
fn terminal(_: &RuntimePaths, _: &args::Exec) -> Result<i32, String> {
    Err("Container computers run only on a Linux host.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_owned()).collect()
    }

    fn paths() -> RuntimePaths {
        RuntimePaths {
            guest_image: PathBuf::new(),
            executable: "/nowhere/msb".into(),
            home: "/nowhere/home".into(),
            storage_home: None,
            library: PathBuf::new(),
            metadata: PathBuf::new(),
            volumes: PathBuf::new(),
        }
    }

    #[test]
    fn only_the_ssh_server_and_a_terminal_are_answered() {
        for refused in [
            args(&["start", "box"]),
            args(&["exec", "box", "--", "id"]),
            args(&["inspect", "box"]),
        ] {
            let error = run_with(&paths(), &refused).unwrap_err();
            assert!(error.contains("not available"), "{refused:?}: {error}");
        }
    }

    #[test]
    fn a_server_for_a_computer_that_does_not_exist_says_so() {
        let error = run_with(&paths(), &args(&["ssh", "serve", "ghost", "--stdio"])).unwrap_err();
        assert!(error.contains("not found") || error.contains("Linux host"), "{error}");
    }
}
