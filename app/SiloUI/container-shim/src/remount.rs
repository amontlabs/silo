//! `silo-ctr remount --state DIR --ro|--rw`: changes the container's root file system between
//! read-only and read-write, from outside.
//!
//! The engine uses it to hold a computer to its disk size: a rootless host has no quota to set
//! on a writable layer (that needs administrator rights on the file system), so the engine
//! measures the layer and, past the limit, makes the root read-only. The command joins the holder's
//! user namespace and the container's mount namespace and remounts `/` there; the container's other
//! mounts (its share, `/tmp`, `/dev`) stay writable.

/// The `--state` and mode options: `true` for read-only.
pub fn parse_args(args: &[String]) -> Result<(String, bool), String> {
    let usage = "usage: silo-ctr remount --state DIR --ro|--rw";
    match args {
        [flag, state, mode] if flag == "--state" => match mode.as_str() {
            "--ro" => Ok((state.clone(), true)),
            "--rw" => Ok((state.clone(), false)),
            _ => Err(usage.into()),
        },
        _ => Err(usage.into()),
    }
}

#[cfg(target_os = "linux")]
pub fn command(args: &[String]) -> Result<i32, String> {
    use crate::common::{io_err, os_err};
    use std::os::fd::AsRawFd;

    let (state, read_only) = parse_args(args)?;
    let pid = std::fs::read_to_string(std::path::Path::new(&state).join("container.pid"))
        .map_err(|error| io_err("read container.pid", &error))?
        .trim()
        .parse::<u32>()
        .map_err(|_| "container.pid does not hold a process id".to_owned())?;
    // The user namespace first, as every command that reaches into the holder does.
    crate::enter::join_holder(&state)?;
    let namespace = std::fs::File::open(format!("/proc/{pid}/ns/mnt"))
        .map_err(|error| io_err("open the container's mount namespace", &error))?;
    // SAFETY: a valid namespace descriptor and flag; the process has one thread.
    if unsafe { libc::setns(namespace.as_raw_fd(), libc::CLONE_NEWNS) } != 0 {
        return Err(os_err("join the container's mount namespace"));
    }
    let flags = libc::MS_REMOUNT | libc::MS_BIND | if read_only { libc::MS_RDONLY } else { 0 };
    // SAFETY: the root path is a valid NUL-terminated string; the other pointers may be null.
    let rc = unsafe {
        libc::mount(
            std::ptr::null(),
            c"/".as_ptr(),
            std::ptr::null(),
            flags,
            std::ptr::null(),
        )
    };
    if rc != 0 {
        return Err(os_err("remount the container's root"));
    }
    Ok(0)
}

#[cfg(not(target_os = "linux"))]
pub fn command(_: &[String]) -> Result<i32, String> {
    Err("Linux only".into())
}

#[cfg(test)]
mod tests {
    use super::parse_args;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_state_folder_and_a_mode_are_required() {
        assert_eq!(
            parse_args(&args(&["--state", "/s", "--ro"])).unwrap(),
            ("/s".to_owned(), true)
        );
        assert_eq!(
            parse_args(&args(&["--state", "/s", "--rw"])).unwrap(),
            ("/s".to_owned(), false)
        );
        assert!(parse_args(&args(&["--state", "/s"])).is_err());
        assert!(parse_args(&args(&["--state", "/s", "--x"])).is_err());
        assert!(parse_args(&args(&[])).is_err());
    }
}
