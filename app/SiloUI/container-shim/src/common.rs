//! Error text, signal forwarding and wait-status decoding shared by both binaries.

use std::ffi::CStr;
use std::sync::atomic::{AtomicI32, Ordering};

/// The text of an errno value (`strerror`).
pub fn errno_text(errno: i32) -> String {
    let mut buf = [0 as libc::c_char; 256];
    // SAFETY: `buf` is valid for `buf.len()` bytes; the XSI strerror_r NUL-terminates it on success.
    let rc = unsafe { libc::strerror_r(errno, buf.as_mut_ptr(), buf.len()) };
    if rc != 0 {
        return format!("error {errno}");
    }
    // SAFETY: strerror_r succeeded, so `buf` holds a NUL-terminated string.
    unsafe { CStr::from_ptr(buf.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

pub fn last_errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// `what: strerror(errno)` for the calling thread's last failed call.
pub fn os_err(what: &str) -> String {
    format!("{what}: {}", errno_text(last_errno()))
}

/// `what: strerror` for an I/O error that carries an OS error code, else its own text.
pub fn io_err(what: &str, error: &std::io::Error) -> String {
    match error.raw_os_error() {
        Some(code) => format!("{what}: {}", errno_text(code)),
        None => format!("{what}: {error}"),
    }
}

/// The process exit code for a `waitpid` status: the exit status, or 128 plus the signal.
pub fn status_code(status: i32) -> i32 {
    if libc::WIFEXITED(status) {
        libc::WEXITSTATUS(status)
    } else if libc::WIFSIGNALED(status) {
        128 + libc::WTERMSIG(status)
    } else {
        1
    }
}

/// The kernel's Landlock ABI, or the errno text when Landlock is unavailable.
#[cfg(target_os = "linux")]
pub fn landlock_abi() -> Result<u32, String> {
    const SYS_CREATE_RULESET: libc::c_long = 444;
    const CREATE_RULESET_VERSION: u32 = 1;
    // SAFETY: with the VERSION flag the attribute pointer must be null and its size 0.
    let rc = unsafe {
        libc::syscall(
            SYS_CREATE_RULESET,
            std::ptr::null::<u8>(),
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

/// The first Landlock ABI that also restricts network access (TCP bind and connect).
pub const LANDLOCK_NETWORK_ABI: u32 = 4;

/// What the holder logs once per start about the filesystem ruleset `silo-init` applies, or
/// `None` when the kernel is new enough (ABI 4 or later) for everything the baseline asks for.
pub fn landlock_warning(abi: &Result<u32, String>) -> Option<String> {
    match abi {
        Err(reason) => Some(format!(
            "Landlock is unavailable ({reason}): the container runs without its filesystem ruleset, which protects only /silo/init, /boot and the root directory. The namespaces, capabilities, seccomp and read-only masks still apply."
        )),
        Ok(abi) if *abi < LANDLOCK_NETWORK_ABI => Some(format!(
            "Landlock ABI {abi} is older than {LANDLOCK_NETWORK_ABI}: the filesystem ruleset applies, but this kernel is older than the baseline expects; update the host kernel."
        )),
        Ok(_) => None,
    }
}

static TARGET: AtomicI32 = AtomicI32::new(0);

extern "C" fn forward(signal: libc::c_int) {
    let target = TARGET.load(Ordering::Relaxed);
    if target > 0 {
        // SAFETY: kill is async-signal-safe and takes plain integers.
        unsafe { libc::kill(target, signal) };
    }
}

/// Sets the process the forwarded signals go to.
pub fn set_forward_target(pid: i32) {
    TARGET.store(pid, Ordering::Relaxed);
}

/// Installs a handler for each signal that passes it to the forward target (none until set).
pub fn install_forwarding(signals: &[libc::c_int]) -> Result<(), String> {
    for &signal in signals {
        // SAFETY: a zeroed sigaction is a valid starting value; the handler is an async-signal-safe
        // function and the mask is emptied before use.
        let rc = unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = forward as extern "C" fn(libc::c_int) as usize;
            action.sa_flags = libc::SA_RESTART;
            libc::sigemptyset(&mut action.sa_mask);
            libc::sigaction(signal, &action, std::ptr::null_mut())
        };
        if rc != 0 {
            return Err(os_err("sigaction"));
        }
    }
    Ok(())
}

/// Waits for `pid`, retrying when a signal interrupts; returns its wait status.
pub fn wait_for(pid: i32) -> Result<i32, String> {
    loop {
        let mut status = 0;
        // SAFETY: `status` is a valid out pointer.
        let got = unsafe { libc::waitpid(pid, &mut status, 0) };
        if got == pid {
            return Ok(status);
        }
        if got < 0 && last_errno() == libc::ECHILD {
            return Err("waitpid: no such child".into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errno_text_names_the_error() {
        assert_eq!(errno_text(libc::ENOENT), "No such file or directory");
    }

    #[test]
    fn a_missing_or_old_landlock_is_warned_about_and_a_current_one_is_not() {
        let missing = landlock_warning(&Err("Function not implemented".into())).unwrap();
        assert!(missing.contains("unavailable") && missing.contains("Function not implemented"));
        for old in [1, 2, 3] {
            let warning = landlock_warning(&Ok(old)).unwrap();
            assert!(warning.contains(&format!("ABI {old}")), "{warning}");
        }
        assert_eq!(landlock_warning(&Ok(4)), None);
        assert_eq!(landlock_warning(&Ok(7)), None);
    }

    #[test]
    fn status_code_decodes_exit_and_signal() {
        assert_eq!(status_code(3 << 8), 3);
        assert_eq!(status_code(libc::SIGTERM), 128 + libc::SIGTERM);
    }
}
