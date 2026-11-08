//! `silo-ctr run`: the container holder. It enters a new user, mount and network namespace, mounts the
//! overlay root, sets up the network, then becomes `crun run`. The parent stays behind as the holder:
//! it records its pid and the namespace owner's pid in the state directory and exits with crun's status.

use crate::userns::number_after;

#[derive(Debug, PartialEq)]
pub struct RunArgs {
    pub state: String,
    pub lower: Vec<String>,
    pub upper: String,
    pub work: String,
    pub merged: String,
    pub bundle: String,
    pub id: String,
    pub sock: Option<String>,
    pub crun: String,
    pub sub_uid: Option<u32>,
    pub sub_gid: Option<u32>,
}

pub fn parse_args(args: &[String]) -> Result<RunArgs, String> {
    let mut values: Vec<(String, String)> = Vec::new();
    let (mut sub_uid, mut sub_gid) = (None, None);
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        index += 1;
        let name = flag.strip_prefix("--").unwrap_or("");
        if !matches!(
            name,
            "state"
                | "lower"
                | "upper"
                | "work"
                | "merged"
                | "bundle"
                | "id"
                | "sock"
                | "crun"
                | "sub-uid"
                | "sub-gid"
        ) {
            return Err(format!("unknown option {flag}"));
        }
        match name {
            "sub-uid" => sub_uid = Some(number_after(flag, args.get(index))?),
            "sub-gid" => sub_gid = Some(number_after(flag, args.get(index))?),
            _ => {
                let value = args
                    .get(index)
                    .ok_or_else(|| format!("{flag} needs a value"))?;
                values.retain(|(known, _)| known != name);
                values.push((name.to_string(), value.clone()));
            }
        }
        index += 1;
    }
    let take = |name: &str| {
        values
            .iter()
            .find(|(known, _)| known == name)
            .map(|(_, value)| value.clone())
    };
    let need = |name: &str| take(name).ok_or_else(|| format!("--{name} is required"));
    let lower: Vec<String> = need("lower")?.split(':').map(str::to_string).collect();
    Ok(RunArgs {
        state: need("state")?,
        lower,
        upper: need("upper")?,
        work: need("work")?,
        merged: need("merged")?,
        bundle: need("bundle")?,
        id: need("id")?,
        sock: take("sock"),
        crun: take("crun").unwrap_or_else(|| "crun".to_string()),
        sub_uid,
        sub_gid,
    })
}

fn check_path(role: &str, path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err(format!("{role} path is empty"));
    }
    if path.contains([',', ':', '\0']) {
        return Err(format!(
            "{role} path {path:?} contains ':' or ',' or NUL, which overlayfs options cannot hold"
        ));
    }
    Ok(())
}

/// The overlay mount options; the lower directories are listed top layer first, as the kernel wants.
pub fn overlay_options(lower: &[String], upper: &str, work: &str) -> Result<String, String> {
    if lower.is_empty() {
        return Err("lower is empty".into());
    }
    for path in lower {
        check_path("lower", path)?;
    }
    check_path("upper", upper)?;
    check_path("work", work)?;
    Ok(format!(
        "lowerdir={},upperdir={upper},workdir={work},userxattr",
        lower.join(":")
    ))
}

/// The `crun` command line; its state lives under the holder's state directory.
pub fn crun_command(args: &RunArgs) -> Vec<String> {
    let state = args.state.trim_end_matches('/');
    [
        args.crun.as_str(),
        "--root",
        &format!("{state}/crun"),
        "--cgroup-manager=cgroupfs",
        "run",
        "--bundle",
        &args.bundle,
        "--pid-file",
        &format!("{state}/container.pid"),
        &args.id,
    ]
    .iter()
    .map(|part| part.to_string())
    .collect()
}

#[cfg(target_os = "linux")]
pub use linux::command;

#[cfg(target_os = "linux")]
mod linux {
    use super::{crun_command, overlay_options, parse_args, RunArgs};
    use crate::common::{io_err, os_err};
    use crate::{net, sock, userns};
    use std::convert::Infallible;
    use std::ffi::CString;
    use std::fs::File;
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    use std::path::Path;

    /// Writes `<state>/<name>` (mode 0600) through a temporary file and a rename.
    fn write_pid(state: &str, name: &str, pid: i32) -> Result<(), String> {
        let final_path = Path::new(state).join(name);
        let temporary = Path::new(state).join(format!(".{name}.tmp"));
        let mut file = File::options()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|error| io_err(&format!("create {}", temporary.display()), &error))?;
        file.write_all(format!("{pid}\n").as_bytes())
            .map_err(|error| io_err(&format!("write {}", temporary.display()), &error))?;
        std::fs::rename(&temporary, &final_path)
            .map_err(|error| io_err(&format!("rename {}", final_path.display()), &error))
    }

    fn mount_overlay(options: &str, target: &str) -> Result<(), String> {
        let (Ok(options), Ok(target)) = (CString::new(options), CString::new(target)) else {
            return Err("an overlay path contains a NUL byte".into());
        };
        // SAFETY: every pointer is a live NUL-terminated string for the duration of the call.
        let rc = unsafe {
            libc::mount(
                c"overlay".as_ptr(),
                target.as_ptr(),
                c"overlay".as_ptr(),
                0,
                options.as_ptr() as *const libc::c_void,
            )
        };
        if rc != 0 {
            return Err(os_err(&format!(
                "mount overlay on {}",
                target.to_string_lossy()
            )));
        }
        Ok(())
    }

    /// Creates the TAP device, hands it to the engine over the socket and configures it as told.
    fn network_over(path: &str) -> Result<(), String> {
        let tap = net::create_tap()?;
        let stream = sock::connect(path)?;
        sock::send_tap(&stream, tap.as_raw_fd())?;
        drop(tap);
        let config = sock::read_config(&stream)?;
        net::configure_tap(&config)?;
        sock::send_up(&stream)
    }

    /// The part inside the namespaces: root filesystem, network, then crun.
    fn inside(args: &RunArgs, options: &str) -> Result<Infallible, String> {
        mount_overlay(options, &args.merged)?;
        net::loopback_up()?;
        if let Some(path) = &args.sock {
            network_over(path)?;
        }
        userns::exec_command(&crun_command(args))
    }

    /// `silo-ctr run ...`; returns crun's exit code.
    pub fn command(arguments: &[String]) -> Result<i32, String> {
        let args = parse_args(arguments)?;
        let options = overlay_options(&args.lower, &args.upper, &args.work)?;
        let spec = userns::resolve(args.sub_uid, args.sub_gid, true)?;
        let crun_root = Path::new(&args.state).join("crun");
        std::fs::create_dir_all(&crun_root)
            .map_err(|error| io_err(&format!("create {}", crun_root.display()), &error))?;
        // SAFETY: getpid has no preconditions.
        write_pid(&args.state, "holder.pid", unsafe { libc::getpid() })?;
        let outcome = userns::launch(
            &spec,
            &mut |pid| write_pid(&args.state, "outer.pid", pid),
            || inside(&args, &options),
        );
        for name in ["holder.pid", "outer.pid"] {
            let _ = std::fs::remove_file(Path::new(&args.state).join(name));
        }
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    fn full() -> Vec<String> {
        args(&[
            "--state", "/s", "--lower", "/l1:/l2", "--upper", "/u", "--work", "/w", "--merged",
            "/m", "--bundle", "/b", "--id", "c1",
        ])
    }

    #[test]
    fn parses_the_required_and_optional_options() {
        let mut list = full();
        list.extend(args(&[
            "--sock",
            "/tmp/x.sock",
            "--crun",
            "/usr/bin/crun",
            "--sub-uid",
            "100000",
            "--sub-gid",
            "200000",
        ]));
        let parsed = parse_args(&list).unwrap();
        assert_eq!(parsed.lower, args(&["/l1", "/l2"]));
        assert_eq!(parsed.sock.as_deref(), Some("/tmp/x.sock"));
        assert_eq!(
            (parsed.crun.as_str(), parsed.sub_uid, parsed.sub_gid),
            ("/usr/bin/crun", Some(100000), Some(200000))
        );
        let plain = parse_args(&full()).unwrap();
        assert_eq!(
            (plain.sock, plain.crun.as_str(), plain.sub_uid),
            (None, "crun", None)
        );
    }

    #[test]
    fn bad_input_is_an_error() {
        assert!(parse_args(&[]).unwrap_err().contains("is required"));
        let mut missing = full();
        missing.truncate(missing.len() - 2);
        assert_eq!(parse_args(&missing).unwrap_err(), "--id is required");
        assert!(parse_args(&args(&["--state"]))
            .unwrap_err()
            .contains("needs a value"));
        assert!(parse_args(&args(&["--bogus", "x"]))
            .unwrap_err()
            .contains("--bogus"));
        assert!(parse_args(&args(&["state"])).is_err());
        let mut bad = full();
        bad.extend(args(&["--sub-uid", "many"]));
        assert!(parse_args(&bad).unwrap_err().contains("not a number"));
    }

    #[test]
    fn the_overlay_options_keep_the_given_layer_order() {
        assert_eq!(
            overlay_options(&args(&["/boot", "/rootfs"]), "/u", "/w").unwrap(),
            "lowerdir=/boot:/rootfs,upperdir=/u,workdir=/w,userxattr"
        );
    }

    #[test]
    fn a_path_that_overlayfs_cannot_hold_is_refused() {
        for bad in ["/a,b", "/a:b", "", "/a\0b"] {
            assert!(
                overlay_options(&args(&[bad]), "/u", "/w").is_err(),
                "{bad:?}"
            );
            assert!(
                overlay_options(&args(&["/l"]), bad, "/w").is_err(),
                "{bad:?}"
            );
            assert!(
                overlay_options(&args(&["/l"]), "/u", bad).is_err(),
                "{bad:?}"
            );
        }
        assert!(overlay_options(&[], "/u", "/w").is_err());
        let mut list = full();
        list[3] = "/l1::/l2".to_string();
        assert!(overlay_options(&parse_args(&list).unwrap().lower, "/u", "/w").is_err());
    }

    #[test]
    fn the_crun_command_line_is_fixed() {
        let mut list = full();
        list[1] = "/s/".to_string();
        let parsed = parse_args(&list).unwrap();
        assert_eq!(
            crun_command(&parsed),
            args(&[
                "crun",
                "--root",
                "/s/crun",
                "--cgroup-manager=cgroupfs",
                "run",
                "--bundle",
                "/b",
                "--pid-file",
                "/s/container.pid",
                "c1"
            ])
        );
    }
}
