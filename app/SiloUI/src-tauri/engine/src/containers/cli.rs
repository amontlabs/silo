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
        // The export and import of a container computer run as a child of the engine, like
        // MicroSandbox's snapshot commands, so that they can be timed out and cancelled.
        Command::SnapshotCreate(_)
        | Command::SnapshotList { .. }
        | Command::SnapshotVerify { .. }
        | Command::SnapshotRemove { .. }
        | Command::SnapshotHead { .. }
        | Command::SnapshotSave(_)
        | Command::SnapshotLoad(_) => {
            let output = super::run(
                paths,
                arguments,
                std::time::Duration::from_secs(60 * 60 * 24),
                &super::Material::NONE,
            )
            .map_err(|error| error.to_string())?;
            print!("{}", output.stdout);
            Ok(0)
        }
        _ => Err("This command is not available for container computers.".into()),
    }
}

/// A terminal in the computer, on the terminal this process is attached to. `crun` makes the
/// pseudo-terminal and hands its controlling side to a private socket here; this process puts
/// its own terminal in raw mode and copies bytes both ways until the command ends, passing the
/// window size along.
#[cfg(target_os = "linux")]
fn terminal(paths: &RuntimePaths, exec: &args::Exec) -> Result<i32, String> {
    use super::{lifecycle, tools::Tools};
    use std::os::fd::AsRawFd;
    use std::process::Stdio;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    static RESIZED: AtomicBool = AtomicBool::new(true);
    extern "C" fn on_resize(_: libc::c_int) {
        RESIZED.store(true, Ordering::Relaxed);
    }

    let tools = Tools::locate(paths).map_err(|error| error.to_string())?;
    if !lifecycle::holder_alive(paths, &exec.name) {
        return Err(format!("Start {} first.", exec.name));
    }
    let directory = super::hold::console_directory()
        .map_err(|error| format!("The terminal could not start: {error}"))?;
    let socket = directory.path().join("c");
    let listener = std::os::unix::net::UnixListener::bind(&socket)
        .map_err(|error| format!("The terminal could not start: {error}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let mut with_socket = exec.clone();
    with_socket.console_socket = Some(socket);
    let lifecycle::ExecPlan { command, group } =
        lifecycle::exec_command(paths, &tools, &exec.name, &with_socket, false)?;
    // A terminal is started from a login session, not from the engine's control group.
    let mut command = lifecycle::in_user_scope(&tools, &command);
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .map_err(|error| format!("The terminal could not start: {error}"))?;
    let deadline = Instant::now() + Duration::from_secs(15);
    let master = loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream
                    .set_nonblocking(false)
                    .map_err(|error| error.to_string())?;
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .map_err(|error| error.to_string())?;
                break lifecycle::receive_descriptor(&stream)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if let Ok(Some(status)) = child.try_wait() {
                    return Err(format!("The terminal could not start ({status})."));
                }
                if Instant::now() >= deadline {
                    group.kill();
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("The terminal did not start in time.".into());
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error.to_string()),
        }
    };
    drop(directory);
    let master = std::fs::File::from(master);
    let master_fd = master.as_raw_fd();
    // SAFETY: plain calls on descriptors this process owns; `saved` is only used when it was filled.
    let interactive = unsafe { libc::isatty(0) == 1 };
    let mut saved: libc::termios = unsafe { std::mem::zeroed() };
    let raw_mode = interactive && unsafe { libc::tcgetattr(0, &mut saved) } == 0;
    if raw_mode {
        let mut raw = saved;
        // SAFETY: `raw` is a valid termios.
        unsafe {
            libc::cfmakeraw(&mut raw);
            libc::tcsetattr(0, libc::TCSANOW, &raw);
            libc::signal(libc::SIGWINCH, on_resize as libc::sighandler_t);
        }
    }
    let mut stdin_open = true;
    let mut buffer = [0u8; 8192];
    let status = loop {
        if RESIZED.swap(false, Ordering::Relaxed) && interactive {
            // SAFETY: TIOCGWINSZ/TIOCSWINSZ read and write one winsize.
            unsafe {
                let mut size: libc::winsize = std::mem::zeroed();
                if libc::ioctl(0, libc::TIOCGWINSZ, &mut size) == 0 {
                    libc::ioctl(master_fd, libc::TIOCSWINSZ, &size);
                }
            }
        }
        let mut fds = [
            libc::pollfd {
                fd: if stdin_open { 0 } else { -1 },
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: master_fd,
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: `fds` is a valid array of two pollfd.
        unsafe { libc::poll(fds.as_mut_ptr(), 2, 100) };
        if fds[0].revents & (libc::POLLIN | libc::POLLHUP) != 0 {
            // SAFETY: reads into a buffer of the length given.
            let read = unsafe { libc::read(0, buffer.as_mut_ptr().cast(), buffer.len()) };
            if read > 0 {
                write_all(master_fd, &buffer[..read as usize]);
            } else if read == 0 {
                stdin_open = false;
            }
        }
        if fds[1].revents & (libc::POLLIN | libc::POLLHUP) != 0 {
            // SAFETY: reads into a buffer of the length given.
            let read = unsafe { libc::read(master_fd, buffer.as_mut_ptr().cast(), buffer.len()) };
            if read > 0 {
                write_all(1, &buffer[..read as usize]);
            } else if read <= 0 {
                // The command closed its terminal: it is over.
                break child.wait().map_err(|error| error.to_string());
            }
        }
        if let Ok(Some(status)) = child.try_wait() {
            // Whatever it printed last.
            loop {
                let mut pending = libc::pollfd {
                    fd: master_fd,
                    events: libc::POLLIN,
                    revents: 0,
                };
                // SAFETY: one valid pollfd and a read into a buffer of the length given.
                let ready = unsafe { libc::poll(&mut pending, 1, 50) };
                if ready <= 0 {
                    break;
                }
                let read =
                    unsafe { libc::read(master_fd, buffer.as_mut_ptr().cast(), buffer.len()) };
                if read <= 0 {
                    break;
                }
                write_all(1, &buffer[..read as usize]);
            }
            break Ok(status);
        }
    };
    if raw_mode {
        // SAFETY: restores the attributes read above.
        unsafe { libc::tcsetattr(0, libc::TCSANOW, &saved) };
    }
    group.kill();
    let status = status?;
    use std::os::unix::process::ExitStatusExt;
    Ok(status
        .code()
        .or_else(|| status.signal().map(|signal| 128 + signal))
        .unwrap_or(1))
}

/// Writes all of `bytes` to `fd`, giving up on an error.
#[cfg(target_os = "linux")]
fn write_all(fd: i32, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        // SAFETY: writes from a slice of the length given.
        let written = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
        if written <= 0 {
            return;
        }
        bytes = &bytes[written as usize..];
    }
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
    fn only_the_ssh_server_a_terminal_and_the_snapshot_commands_are_answered() {
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
        assert!(
            error.contains("not found") || error.contains("Linux host"),
            "{error}"
        );
    }
}
