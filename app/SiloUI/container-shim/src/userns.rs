//! `silo-ctr userns`: a command as root of a new user and mount namespace (and network namespace with
//! `--net`), the way rootless podman sets up its outer namespace. Container id 0 is the caller, ids
//! 1..=65535 the caller's sub-id range, mapped by the setuid `newuidmap` and `newgidmap`. With `--keep-id N`
//! the caller is id N instead (as rootless podman's `--userns=keep-id`), the sub-id range fills the other ids, and
//! the command runs as id 0, a sub-id: the layout a container computer's files are owned by.

use crate::subid;

#[derive(Debug, PartialEq)]
pub struct UsernsArgs {
    pub sub_uid: Option<u32>,
    pub sub_gid: Option<u32>,
    pub net: bool,
    /// The id the caller keeps (the container's working account), instead of becoming root.
    pub keep_id: Option<u32>,
    pub command: Vec<String>,
}

/// The value after `flag`, as a number.
pub fn number_after(flag: &str, value: Option<&String>) -> Result<u32, String> {
    let value = value.ok_or_else(|| format!("{flag} needs a value"))?;
    value
        .parse()
        .map_err(|_| format!("{flag}: {value:?} is not a number"))
}

pub fn parse_args(args: &[String]) -> Result<UsernsArgs, String> {
    let mut parsed = UsernsArgs {
        sub_uid: None,
        sub_gid: None,
        net: false,
        keep_id: None,
        command: Vec::new(),
    };
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        index += 1;
        match arg {
            "--" => {
                parsed.command = args[index..].to_vec();
                break;
            }
            "--net" => parsed.net = true,
            "--keep-id" => {
                let id = number_after(arg, args.get(index))?;
                index += 1;
                if !(1..subid::RANGE).contains(&id) {
                    return Err(format!(
                        "--keep-id: {id} is not between 1 and {}",
                        subid::RANGE - 1
                    ));
                }
                parsed.keep_id = Some(id);
            }
            "--sub-uid" | "--sub-gid" => {
                let number = number_after(arg, args.get(index))?;
                index += 1;
                if arg == "--sub-uid" {
                    parsed.sub_uid = Some(number);
                } else {
                    parsed.sub_gid = Some(number);
                }
            }
            _ => return Err(format!("unknown option {arg}")),
        }
    }
    if parsed.command.is_empty() {
        return Err("no command".into());
    }
    Ok(parsed)
}

/// The `newuidmap`/`newgidmap` arguments after the pid: container 0 is `id`, 1..=65535 start at `sub`.
/// With `keep` the caller `id` is container `keep` instead; container 0..keep and keep+1..=65535 take the
/// sub-ids in order, so a container id `c` below `keep` is sub-id `sub + c` and one above it `sub + c - 1`.
pub fn map_args(id: u32, sub: u32, keep: Option<u32>) -> Vec<String> {
    let extents: Vec<(u32, u32, u32)> = match keep {
        None => vec![(0, id, 1), (1, sub, subid::RANGE)],
        Some(keep) => vec![
            (0, sub, keep),
            (keep, id, 1),
            (keep + 1, sub + keep, subid::RANGE - keep),
        ],
    };
    extents
        .into_iter()
        .flat_map(|(inside, outside, count)| [inside, outside, count])
        .map(|number| number.to_string())
        .collect()
}

#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(target_os = "linux")]
mod linux {
    use super::{map_args, parse_args};
    use crate::common::{
        install_forwarding, io_err, os_err, set_forward_target, status_code, wait_for,
    };
    use crate::subid;
    use std::convert::Infallible;
    use std::fs::File;
    use std::io::{Read, Write};
    use std::os::fd::{FromRawFd, OwnedFd};
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    /// The namespaces to enter and the sub-id ranges to map into them.
    pub struct Spec {
        pub sub_uid: u32,
        pub sub_gid: u32,
        pub net: bool,
        pub keep_id: Option<u32>,
    }

    /// The caller's ranges, or the explicitly given starts.
    pub fn resolve(
        sub_uid: Option<u32>,
        sub_gid: Option<u32>,
        net: bool,
        keep_id: Option<u32>,
    ) -> Result<Spec, String> {
        // SAFETY: getuid and getgid have no preconditions and cannot fail.
        let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
        let user = subid::user_name(uid);
        let sub_uid = match sub_uid {
            Some(start) => start,
            None => subid::range_from_file("/etc/subuid", &user, uid)?,
        };
        let sub_gid = match sub_gid {
            Some(start) => start,
            None => subid::range_from_file("/etc/subgid", &user, gid)?,
        };
        Ok(Spec {
            sub_uid,
            sub_gid,
            net,
            keep_id,
        })
    }

    fn pipe() -> Result<(File, File), String> {
        let mut fds = [0 as libc::c_int; 2];
        // SAFETY: `fds` has room for the two descriptors pipe2 writes.
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
            return Err(os_err("pipe"));
        }
        // SAFETY: pipe2 returned two fresh descriptors that nothing else owns.
        let (read, write) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
        Ok((File::from(read), File::from(write)))
    }

    /// Makes every mount of this mount namespace private, so nothing propagates to the host.
    pub fn make_private() -> Result<(), String> {
        // SAFETY: null source, fstype and data are allowed for a propagation change; "/" is NUL-terminated.
        let rc = unsafe {
            libc::mount(
                std::ptr::null(),
                c"/".as_ptr(),
                std::ptr::null(),
                libc::MS_REC | libc::MS_PRIVATE,
                std::ptr::null(),
            )
        };
        if rc != 0 {
            return Err(os_err("mount --make-rprivate /"));
        }
        Ok(())
    }

    /// Replaces the process with the command (searched in PATH).
    pub fn exec_command(command: &[String]) -> Result<Infallible, String> {
        let error = Command::new(&command[0]).args(&command[1..]).exec();
        Err(io_err(&format!("exec {}", command[0]), &error))
    }

    fn run_map(
        program: &str,
        pid: i32,
        id: u32,
        sub: u32,
        keep: Option<u32>,
    ) -> Result<(), String> {
        let status = Command::new(program)
            .arg(pid.to_string())
            .args(map_args(id, sub, keep))
            .status()
            .map_err(|error| io_err(program, &error))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("{program} failed: {status}"))
        }
    }

    fn kill_and_reap(pid: i32) {
        // SAFETY: SIGKILL to our own child.
        unsafe { libc::kill(pid, libc::SIGKILL) };
        let _ = wait_for(pid);
    }

    /// The forked child: enters the namespaces, waits for the parent to map the ids, then runs `body`.
    fn child_side(
        spec: &Spec,
        mut ready: File,
        mut go: File,
        body: impl FnOnce() -> Result<Infallible, String>,
    ) -> Result<Infallible, String> {
        let mut flags = libc::CLONE_NEWUSER | libc::CLONE_NEWNS;
        if spec.net {
            flags |= libc::CLONE_NEWNET;
        }
        // SAFETY: the child is single-threaded, as unshare(CLONE_NEWUSER) requires.
        if unsafe { libc::unshare(flags) } != 0 {
            let error = os_err("unshare");
            let _ = ready.write_all(b"E");
            return Err(error);
        }
        ready
            .write_all(b"K")
            .map_err(|error| io_err("write to parent", &error))?;
        let mut answer = [0u8; 1];
        match go.read(&mut answer) {
            Ok(1) if answer[0] == b'G' => {}
            _ => return Err("the parent did not finish the id mapping".into()),
        }
        drop((ready, go));
        if spec.keep_id.is_some() {
            // The caller is not root here: become root (a sub-id) so the command has the capabilities
            // the namespace gave it and creates files as root.
            // SAFETY: plain id changes of this single-threaded child, with CAP_SETUID/SETGID in its namespace.
            if unsafe { libc::setresgid(0, 0, 0) } != 0 || unsafe { libc::setresuid(0, 0, 0) } != 0
            {
                return Err(os_err("become root in the namespace"));
            }
        }
        make_private()?;
        body()
    }

    /// Forks a child into new namespaces and maps the ids, then calls `body` in the child. `after_fork` runs
    /// in the parent with the child's pid before the mapping. Forwards SIGTERM, SIGINT and SIGHUP to the
    /// child; returns its exit code (128 plus the signal when it was killed).
    pub fn launch(
        spec: &Spec,
        after_fork: &mut dyn FnMut(i32) -> Result<(), String>,
        body: impl FnOnce() -> Result<Infallible, String>,
    ) -> Result<i32, String> {
        let signals = [libc::SIGTERM, libc::SIGINT, libc::SIGHUP];
        let (ready_read, ready_write) = pipe()?;
        let (go_read, go_write) = pipe()?;
        install_forwarding(&signals)?;
        // SAFETY: the process is single-threaded here; the child only runs this crate's code before exec.
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            return Err(os_err("fork"));
        }
        if pid == 0 {
            drop((ready_read, go_write));
            for signal in signals {
                // SAFETY: restoring the default action of a signal.
                unsafe { libc::signal(signal, libc::SIG_DFL) };
            }
            let Err(error) = child_side(spec, ready_write, go_read, body);
            eprintln!("silo-ctr: {error}");
            // SAFETY: leaving the forked child without running the parent's exit handlers.
            unsafe { libc::_exit(1) };
        }
        drop((ready_write, go_read));
        set_forward_target(pid);
        let (mut ready, mut go) = (ready_read, go_write);
        if let Err(error) = after_fork(pid) {
            kill_and_reap(pid);
            return Err(error);
        }
        let mut answer = [0u8; 1];
        if !matches!(ready.read(&mut answer), Ok(1)) || answer[0] != b'K' {
            // The child failed and said why on stderr.
            return Ok(status_code(wait_for(pid)?));
        }
        // SAFETY: getuid and getgid have no preconditions and cannot fail.
        let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
        let mapped = run_map("newuidmap", pid, uid, spec.sub_uid, spec.keep_id)
            .and_then(|()| run_map("newgidmap", pid, gid, spec.sub_gid, spec.keep_id));
        if let Err(error) = mapped {
            kill_and_reap(pid);
            return Err(error);
        }
        if let Err(error) = go.write_all(b"G") {
            kill_and_reap(pid);
            return Err(io_err("write to child", &error));
        }
        drop(go);
        Ok(status_code(wait_for(pid)?))
    }

    /// `silo-ctr userns [--sub-uid N] [--sub-gid N] [--net] [--keep-id N] -- CMD ARGS...`
    pub fn command(args: &[String]) -> Result<i32, String> {
        let parsed = parse_args(args)?;
        let spec = resolve(parsed.sub_uid, parsed.sub_gid, parsed.net, parsed.keep_id)?;
        launch(&spec, &mut |_| Ok(()), || exec_command(&parsed.command))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn parses_options_and_keeps_the_commands_own_flags() {
        let parsed = parse_args(&args(&[
            "--sub-uid",
            "100000",
            "--net",
            "--sub-gid",
            "200000",
            "--",
            "id",
            "--net",
        ]))
        .unwrap();
        assert_eq!(
            parsed,
            UsernsArgs {
                sub_uid: Some(100000),
                sub_gid: Some(200000),
                net: true,
                keep_id: None,
                command: args(&["id", "--net"]),
            }
        );
    }

    #[test]
    fn bad_input_is_an_error() {
        assert_eq!(parse_args(&args(&["--net"])).unwrap_err(), "no command");
        assert_eq!(parse_args(&args(&["--"])).unwrap_err(), "no command");
        assert!(parse_args(&args(&["--sub-uid"]))
            .unwrap_err()
            .contains("needs a value"));
        assert!(parse_args(&args(&["--sub-uid", "x", "--", "id"]))
            .unwrap_err()
            .contains("not a number"));
        assert!(parse_args(&args(&["--sub-gid", "-1", "--", "id"])).is_err());
        assert!(parse_args(&args(&["--bogus", "--", "id"]))
            .unwrap_err()
            .contains("--bogus"));
    }

    #[test]
    fn the_map_arguments_put_the_caller_at_zero_and_the_range_at_one() {
        assert_eq!(
            map_args(1000, 165536, None),
            args(&["0", "1000", "1", "1", "165536", "65535"])
        );
    }

    #[test]
    fn keeping_an_id_puts_the_caller_there_and_the_sub_ids_around_it() {
        // Container 0..=1000 are sub-ids 165536.., 1001 is the caller, 1002.. continue the range.
        assert_eq!(
            map_args(1000, 165536, Some(1001)),
            args(&["0", "165536", "1001", "1001", "1000", "1", "1002", "166537", "64534"])
        );
        // The sub-id range is used in full, once.
        let extents = map_args(1000, 165536, Some(1001));
        let counts: u32 = extents
            .chunks(3)
            .map(|e| e[2].parse::<u32>().unwrap())
            .sum();
        assert_eq!(counts, subid::RANGE + 1);
    }

    #[test]
    fn keep_id_is_parsed_and_must_leave_room_for_root() {
        let parsed = parse_args(&args(&["--keep-id", "1001", "--", "id"])).unwrap();
        assert_eq!(parsed.keep_id, Some(1001));
        for bad in ["0", "65535", "70000", "x"] {
            assert!(
                parse_args(&args(&["--keep-id", bad, "--", "id"])).is_err(),
                "{bad}"
            );
        }
    }
}
