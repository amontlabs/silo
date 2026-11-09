//! Starting the guest desktop of a container computer.
//!
//! A virtual machine's init runs the image's boot hook (`silo-desktop-boot`, which starts the
//! desktop unless its automatic start is off). A container has no init system: its PID 1 is
//! Silo's shim and has no capabilities, while the desktop service switches to the working
//! account (`runuser`). So after a start the engine runs the same boot hook once as a root
//! command, which gets the capabilities such commands need (see [`super::oci`]). The service
//! then keeps its own supervisors running, as it does in a virtual machine.
//!
//! The hook waits for the working account first: a first start creates it after the container is
//! up (the service refuses to run without it).

use super::args::Exec;
use std::time::Duration;

/// The root command: waits (at most `ACCOUNT_WAIT_SECONDS`) for the working account, then runs
/// the image's boot hook. A computer whose image has no hook starts nothing.
const SCRIPT: &str = "n=0; while [ ! -f /var/lib/silo/working-account.json ] && [ \"$n\" -lt 150 ]; do sleep 1; n=$((n+1)); done; [ -x /usr/local/libexec/silo-desktop-boot ] || exit 0; exec /usr/local/libexec/silo-desktop-boot";

/// How long the boot command may run (the wait for the account plus the service's own start).
pub const BOOT_TIMEOUT: Duration = Duration::from_secs(300);

/// The command that starts the desktop of container computer `name` if it is set to start.
pub fn boot_command(name: &str) -> Exec {
    Exec {
        name: name.to_owned(),
        user: Some("root".into()),
        env: Vec::new(),
        workdir: Some("/".into()),
        no_start: true,
        timeout: Some(BOOT_TIMEOUT),
        argv: vec!["sh".into(), "-c".into(), SCRIPT.into()],
        ..Exec::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_boot_command_is_a_root_command_that_never_starts_the_computer() {
        let command = boot_command("box");
        assert_eq!(command.name, "box");
        assert_eq!(command.user.as_deref(), Some("root"));
        assert!(command.no_start && !command.tty);
        assert_eq!(command.timeout, Some(BOOT_TIMEOUT));
        assert_eq!(command.argv[..2], ["sh", "-c"]);
    }

    #[test]
    fn the_script_waits_for_the_account_and_runs_the_images_boot_hook() {
        let script = &boot_command("box").argv[2];
        let account = script.find("working-account.json").unwrap();
        let hook = script.find("exec /usr/local/libexec/silo-desktop-boot").unwrap();
        assert!(account < hook);
        // No hook, nothing to start: not an error.
        assert!(script.contains("[ -x /usr/local/libexec/silo-desktop-boot ] || exit 0"));
        // The wait is bounded below the command's timeout.
        assert!(script.contains("-lt 150") && BOOT_TIMEOUT > Duration::from_secs(150));
    }
}
