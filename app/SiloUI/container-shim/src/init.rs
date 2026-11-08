//! silo-init: the container's first process and exec wrapper.
//!
//!   silo-init --probe
//!   silo-init [--require] [--ro=P:P..] [--rw=P:P..] -- CMD ARGS...
//!
//! It restricts the process with a Landlock ruleset (filesystem rights only), then runs the command.
//! As PID 1 it runs the command as a child and stays behind as the reaper of every orphan.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

#[allow(dead_code)]
#[path = "common.rs"]
mod common;

const DEFAULT_RO: &str = "/";
const DEFAULT_RW: &str = "/tmp:/var:/run:/home:/root:/workspace:/dev:/usr:/etc:/opt:/srv:/mnt";

/// The Landlock filesystem rights (`LANDLOCK_ACCESS_FS_*`) up to ABI 3.
mod fs {
    pub const EXECUTE: u64 = 1 << 0;
    pub const WRITE_FILE: u64 = 1 << 1;
    pub const READ_FILE: u64 = 1 << 2;
    pub const READ_DIR: u64 = 1 << 3;
    pub const REMOVE_DIR: u64 = 1 << 4;
    pub const REMOVE_FILE: u64 = 1 << 5;
    pub const MAKE_CHAR: u64 = 1 << 6;
    pub const MAKE_DIR: u64 = 1 << 7;
    pub const MAKE_REG: u64 = 1 << 8;
    pub const MAKE_SOCK: u64 = 1 << 9;
    pub const MAKE_FIFO: u64 = 1 << 10;
    pub const MAKE_BLOCK: u64 = 1 << 11;
    pub const MAKE_SYM: u64 = 1 << 12;
    pub const REFER: u64 = 1 << 13;
    pub const TRUNCATE: u64 = 1 << 14;

    pub const READ_EXEC: u64 = EXECUTE | READ_FILE | READ_DIR;
    /// The rights that apply to something that is not a directory.
    pub const FILE_RIGHTS: u64 = EXECUTE | WRITE_FILE | READ_FILE | TRUNCATE;
    /// The newest ABI this program handles.
    pub const MAX_ABI: u32 = 3;

    /// Every right the ruleset handles for a kernel ABI (capped at `MAX_ABI`).
    pub fn handled(abi: u32) -> u64 {
        let mut mask = EXECUTE
            | WRITE_FILE
            | READ_FILE
            | READ_DIR
            | REMOVE_DIR
            | REMOVE_FILE
            | MAKE_CHAR
            | MAKE_DIR
            | MAKE_REG
            | MAKE_SOCK
            | MAKE_FIFO
            | MAKE_BLOCK
            | MAKE_SYM;
        if abi >= 2 {
            mask |= REFER;
        }
        if abi >= 3 {
            mask |= TRUNCATE;
        }
        mask
    }
}

#[derive(Debug, PartialEq)]
struct Args {
    probe: bool,
    require: bool,
    ro: Vec<String>,
    rw: Vec<String>,
    command: Vec<String>,
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum Access {
    ReadExec,
    Full,
}

fn split_paths(list: &str) -> Vec<String> {
    list.split(':')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect()
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut parsed = Args {
        probe: false,
        require: false,
        ro: split_paths(DEFAULT_RO),
        rw: split_paths(DEFAULT_RW),
        command: Vec::new(),
    };
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        index += 1;
        if arg == "--" {
            parsed.command = args[index..].to_vec();
            break;
        } else if arg == "--probe" {
            parsed.probe = true;
        } else if arg == "--require" {
            parsed.require = true;
        } else if let Some(list) = arg.strip_prefix("--ro=") {
            parsed.ro = split_paths(list);
        } else if let Some(list) = arg.strip_prefix("--rw=") {
            parsed.rw = split_paths(list);
        } else {
            return Err(format!("unknown option {arg}"));
        }
    }
    if !parsed.probe && parsed.command.is_empty() {
        return Err("no command".into());
    }
    Ok(parsed)
}

/// The path rules in the order they are added: read and execute for `ro`, full rights for `rw`.
fn build_rules(ro: &[String], rw: &[String]) -> Vec<(String, Access)> {
    ro.iter()
        .map(|path| (path.clone(), Access::ReadExec))
        .chain(rw.iter().map(|path| (path.clone(), Access::Full)))
        .collect()
}

/// The rights a rule grants on a path: the ruleset's own rights, and only file rights on a non-directory.
fn rights_for(access: Access, is_dir: bool, handled: u64) -> u64 {
    let wanted = match access {
        Access::ReadExec => fs::READ_EXEC,
        Access::Full => handled,
    };
    let granted = wanted & handled;
    if is_dir {
        granted
    } else {
        granted & fs::FILE_RIGHTS
    }
}

#[cfg(target_os = "linux")]
mod landlock {
    use super::{fs, rights_for, Access};
    use crate::common::{errno_text, last_errno, os_err};
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    const SYS_CREATE_RULESET: libc::c_long = 444;
    const SYS_ADD_RULE: libc::c_long = 445;
    const SYS_RESTRICT_SELF: libc::c_long = 446;
    const CREATE_RULESET_VERSION: u32 = 1;
    const RULE_PATH_BENEATH: libc::c_int = 1;

    #[repr(C)]
    struct RulesetAttr {
        handled_access_fs: u64,
    }

    #[repr(C, packed)]
    struct PathBeneathAttr {
        allowed_access: u64,
        parent_fd: i32,
    }

    /// The kernel's Landlock ABI, or the errno text when Landlock is unavailable.
    pub fn abi() -> Result<u32, String> {
        // SAFETY: with the VERSION flag the attribute pointer must be null and its size 0.
        let rc = unsafe {
            libc::syscall(
                SYS_CREATE_RULESET,
                std::ptr::null::<RulesetAttr>(),
                0usize,
                CREATE_RULESET_VERSION,
            )
        };
        if rc < 0 {
            Err(errno_text(last_errno()))
        } else {
            Ok(rc as u32)
        }
    }

    fn add_path(
        ruleset: libc::c_int,
        path: &str,
        access: Access,
        handled: u64,
    ) -> Result<(), String> {
        let Ok(c_path) = CString::new(path.as_bytes()) else {
            return Err(format!("path {path:?} contains a NUL byte"));
        };
        // SAFETY: `c_path` is a valid NUL-terminated string.
        let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_PATH | libc::O_CLOEXEC) };
        if fd < 0 {
            return match last_errno() {
                libc::ENOENT | libc::EACCES | libc::ENOTDIR => Ok(()),
                _ => Err(os_err(&format!("open {path}"))),
            };
        }
        let is_dir = std::fs::metadata(std::ffi::OsStr::from_bytes(path.as_bytes()))
            .map(|meta| meta.is_dir())
            .unwrap_or(true);
        let attr = PathBeneathAttr {
            allowed_access: rights_for(access, is_dir, handled),
            parent_fd: fd,
        };
        // SAFETY: `attr` is a live path_beneath_attr; `ruleset` and `fd` are open descriptors.
        let rc = unsafe {
            libc::syscall(
                SYS_ADD_RULE,
                ruleset,
                RULE_PATH_BENEATH,
                &attr as *const PathBeneathAttr,
                0u32,
            )
        };
        let error = (rc < 0).then(|| os_err(&format!("landlock_add_rule {path}")));
        // SAFETY: `fd` was opened above and is closed once.
        unsafe { libc::close(fd) };
        error.map_or(Ok(()), Err)
    }

    /// Restricts the calling process (and its future children) to the rules.
    pub fn enforce(abi: u32, rules: &[(String, Access)]) -> Result<(), String> {
        let handled = fs::handled(abi.min(fs::MAX_ABI));
        let attr = RulesetAttr {
            handled_access_fs: handled,
        };
        // SAFETY: `attr` is a live ruleset_attr and the size passed is its size.
        let ruleset = unsafe {
            libc::syscall(
                SYS_CREATE_RULESET,
                &attr as *const RulesetAttr,
                std::mem::size_of::<RulesetAttr>(),
                0u32,
            )
        };
        if ruleset < 0 {
            return Err(os_err("landlock_create_ruleset"));
        }
        let ruleset = ruleset as libc::c_int;
        let result = (|| {
            for (path, access) in rules {
                add_path(ruleset, path, *access, handled)?;
            }
            // SAFETY: plain integer arguments.
            if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
                return Err(os_err("prctl PR_SET_NO_NEW_PRIVS"));
            }
            // SAFETY: `ruleset` is a valid ruleset descriptor.
            if unsafe { libc::syscall(SYS_RESTRICT_SELF, ruleset, 0u32) } != 0 {
                return Err(os_err("landlock_restrict_self"));
            }
            Ok(())
        })();
        // SAFETY: `ruleset` was returned by the kernel above and is closed once.
        unsafe { libc::close(ruleset) };
        result
    }
}

#[cfg(target_os = "linux")]
mod run {
    use super::{build_rules, landlock, Args};
    use crate::common::{
        install_forwarding, io_err, last_errno, os_err, set_forward_target, status_code,
    };
    use std::os::unix::process::CommandExt;

    fn fail(message: &str) -> ! {
        eprintln!("silo-init: {message}");
        std::process::exit(1);
    }

    fn probe() -> i32 {
        match landlock::abi() {
            Ok(abi) => println!("landlock abi {abi}"),
            Err(_) => println!("landlock unavailable"),
        }
        0
    }

    fn restrict(args: &Args) {
        match landlock::abi() {
            Ok(abi) => {
                if let Err(error) = landlock::enforce(abi, &build_rules(&args.ro, &args.rw)) {
                    fail(&error);
                }
            }
            Err(reason) => {
                if args.require {
                    fail(&format!("landlock is required but unavailable: {reason}"));
                }
                eprintln!(
                    "silo-init: warning: landlock unavailable ({reason}); running unrestricted"
                );
            }
        }
    }

    /// Replaces the process with the command; only returns the exit code of a failed exec.
    fn exec(command: &[String]) -> i32 {
        let error = std::process::Command::new(&command[0])
            .args(&command[1..])
            .exec();
        eprintln!(
            "silo-init: {}",
            io_err(&format!("exec {}", command[0]), &error)
        );
        127
    }

    /// Runs as PID 1: the command is a child; this process reaps every orphan until the child is gone.
    fn supervise(command: &[String]) -> i32 {
        let signals = [libc::SIGTERM, libc::SIGINT, libc::SIGHUP, libc::SIGQUIT];
        if let Err(error) = install_forwarding(&signals) {
            fail(&error);
        }
        // SAFETY: the process is single-threaded, so the child may run arbitrary code before exec.
        let child = unsafe { libc::fork() };
        if child < 0 {
            fail(&os_err("fork"));
        }
        if child == 0 {
            let code = exec(command);
            // SAFETY: as above.
            unsafe { libc::_exit(code) };
        }
        set_forward_target(child);
        loop {
            let mut status = 0;
            // SAFETY: `status` is a valid out pointer.
            let got = unsafe { libc::waitpid(-1, &mut status, 0) };
            if got == child {
                reap_remaining();
                return status_code(status);
            }
            if got < 0 && last_errno() == libc::ECHILD {
                return 1;
            }
        }
    }

    /// Collects the zombies that are already waiting.
    fn reap_remaining() {
        loop {
            let mut status = 0;
            // SAFETY: `status` is a valid out pointer.
            if unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) } <= 0 {
                return;
            }
        }
    }

    pub fn main(args: &[String]) -> i32 {
        let parsed = match super::parse_args(args) {
            Ok(parsed) => parsed,
            Err(error) => {
                eprintln!("silo-init: {error}");
                return 2;
            }
        };
        if parsed.probe {
            return probe();
        }
        restrict(&parsed);
        // SAFETY: getpid has no preconditions.
        if unsafe { libc::getpid() } == 1 {
            supervise(&parsed.command)
        } else {
            exec(&parsed.command)
        }
    }
}

#[cfg(target_os = "linux")]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run::main(&args));
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("Linux only");
    std::process::exit(2);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn defaults_apply_without_options() {
        let parsed = parse_args(&args(&["--", "/bin/true"])).unwrap();
        assert_eq!(parsed.ro, vec!["/"]);
        assert_eq!(parsed.rw.len(), 12);
        assert_eq!(parsed.rw[0], "/tmp");
        assert_eq!(parsed.rw[11], "/mnt");
        assert_eq!(parsed.command, vec!["/bin/true"]);
        assert!(!parsed.require && !parsed.probe);
    }

    #[test]
    fn options_override_the_defaults_and_the_command_keeps_its_own_flags() {
        let parsed = parse_args(&args(&[
            "--require",
            "--ro=/usr::/lib",
            "--rw=/work",
            "--",
            "sh",
            "-c",
            "--rw=x",
        ]))
        .unwrap();
        assert!(parsed.require);
        assert_eq!(parsed.ro, vec!["/usr", "/lib"]);
        assert_eq!(parsed.rw, vec!["/work"]);
        assert_eq!(parsed.command, vec!["sh", "-c", "--rw=x"]);
    }

    #[test]
    fn probe_needs_no_command() {
        assert!(parse_args(&args(&["--probe"])).unwrap().probe);
    }

    #[test]
    fn bad_input_is_an_error() {
        assert_eq!(parse_args(&args(&[])).unwrap_err(), "no command");
        assert_eq!(parse_args(&args(&["--"])).unwrap_err(), "no command");
        assert!(parse_args(&args(&["--bogus", "--", "x"]))
            .unwrap_err()
            .contains("--bogus"));
        assert!(parse_args(&args(&["x"])).is_err());
    }

    #[test]
    fn rules_put_read_only_paths_before_writable_ones() {
        let rules = build_rules(&args(&["/"]), &args(&["/tmp", "/home"]));
        assert_eq!(
            rules,
            vec![
                ("/".to_string(), Access::ReadExec),
                ("/tmp".to_string(), Access::Full),
                ("/home".to_string(), Access::Full),
            ]
        );
    }

    #[test]
    fn handled_rights_follow_the_abi() {
        assert_eq!(fs::handled(1) & fs::REFER, 0);
        assert_eq!(fs::handled(2) & fs::TRUNCATE, 0);
        assert_ne!(fs::handled(2) & fs::REFER, 0);
        assert_ne!(fs::handled(3) & fs::TRUNCATE, 0);
        assert_eq!(fs::handled(3), (1 << 15) - 1);
    }

    #[test]
    fn a_file_rule_keeps_only_file_rights() {
        let handled = fs::handled(3);
        assert_eq!(rights_for(Access::ReadExec, true, handled), fs::READ_EXEC);
        assert_eq!(rights_for(Access::Full, true, handled), handled);
        assert_eq!(
            rights_for(Access::Full, false, handled),
            fs::EXECUTE | fs::WRITE_FILE | fs::READ_FILE | fs::TRUNCATE
        );
        assert_eq!(
            rights_for(Access::ReadExec, false, handled),
            fs::EXECUTE | fs::READ_FILE
        );
        // An ABI 1 ruleset does not handle truncation, so the rule cannot grant it.
        assert_eq!(
            rights_for(Access::Full, false, fs::handled(1)) & fs::TRUNCATE,
            0
        );
    }
}
