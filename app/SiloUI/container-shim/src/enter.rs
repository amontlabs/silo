//! `silo-ctr enter --state DIR -- CMD ARGS...`: runs a command inside a running container
//! holder's user and network namespaces.
//!
//! `crun exec` joins only the namespaces the container's configuration lists, and the
//! configuration omits the network namespace (the container shares the holder's). Run inside the
//! holder's namespaces, `crun exec` hands its process the same network the container has: the
//! TAP the gateway serves, not the host's.

use crate::common::{io_err, os_err};

#[cfg(target_os = "linux")]
pub fn command(args: &[String]) -> Result<i32, String> {
    use std::{os::fd::AsRawFd, os::unix::process::CommandExt, process::Command};

    let (state, program) = parse(args)?;
    let pid = std::fs::read_to_string(std::path::Path::new(&state).join("outer.pid"))
        .map_err(|error| io_err("read outer.pid", &error))?
        .trim()
        .parse::<u32>()
        .map_err(|_| "outer.pid does not hold a process id".to_owned())?;
    // The user namespace first: it gives the caller the rights to join the namespaces it owns.
    for (file, flag) in [("user", libc::CLONE_NEWUSER), ("net", libc::CLONE_NEWNET)] {
        let namespace = std::fs::File::open(format!("/proc/{pid}/ns/{file}"))
            .map_err(|error| io_err(&format!("open the holder's {file} namespace"), &error))?;
        // SAFETY: a valid namespace descriptor and flag; the process has one thread.
        if unsafe { libc::setns(namespace.as_raw_fd(), flag) } != 0 {
            return Err(os_err(&format!("join the holder's {file} namespace")));
        }
    }
    let error = Command::new(&program[0]).args(&program[1..]).exec();
    Err(io_err(&format!("exec {}", program[0]), &error))
}

#[cfg(not(target_os = "linux"))]
pub fn command(_: &[String]) -> Result<i32, String> {
    Err("Linux only".into())
}

fn parse(args: &[String]) -> Result<(String, Vec<String>), String> {
    let separator = args
        .iter()
        .position(|arg| arg == "--")
        .ok_or("usage: silo-ctr enter --state DIR -- CMD ARGS...")?;
    let state = match &args[..separator] {
        [flag, value] if flag == "--state" => value.clone(),
        _ => return Err("usage: silo-ctr enter --state DIR -- CMD ARGS...".into()),
    };
    let program = args[separator + 1..].to_vec();
    if program.is_empty() {
        return Err("enter needs a command after `--`".into());
    }
    Ok((state, program))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn the_state_directory_and_command_are_required() {
        let (state, program) = parse(&args(&["--state", "/s", "--", "crun", "--root", "x"])).unwrap();
        assert_eq!((state.as_str(), program.len()), ("/s", 3));
        assert!(parse(&args(&["--", "crun"])).is_err());
        assert!(parse(&args(&["--state", "/s", "--"])).is_err());
        assert!(parse(&args(&["--state", "/s"])).is_err());
    }
}
