//! What makes a copy of a template its own computer: the commands that run as root in the
//! copy, over SSH with the template's key, before the copy's own credentials replace it.
//!
//! A copy starts with the template's account password, SSH key and host keys, so any
//! computer made from the same template could log in to it. The script gives the copy a
//! password, key and host keys of its own, a fresh login keychain and its own name.
//! Clone hygiene follows what Lume, Tart and cua-vmm do (a new MAC address and a new
//! machine identifier) plus the guest-side state those tools leave alone.
//!
//! The secrets travel on the standard input of the SSH command: the first line is the new
//! password, the second the new `/etc/kcpassword` content. Neither appears in an argument,
//! in the script or in the environment of any process.
use super::{
    guest_access::{shell_quote, USER},
    offline_setup::{self, GID, UID},
};

const GIB: u64 = 1 << 30;
pub(super) const RESIZED: &str = "SILO_DISK_RESIZE=ok";
pub(super) const NOT_RESIZED: &str = "SILO_DISK_RESIZE=failed";
pub(super) const DONE: &str = "SILO_PERSONALIZED";
const MISSING_SECRETS: i32 = 64;

/// The commands run as root. `grow_by_gib` is how much larger than the template's the disk
/// was made, which the APFS container should grow by. The secrets are only ever read into
/// shell variables and written through the shell's builtin `printf`, so no external process
/// receives them as an argument.
pub(super) fn script(public_key: &str, computer_name: &str, grow_by_gib: Option<u64>) -> String {
    let name = shell_quote(computer_name);
    let authorized_key = shell_quote(public_key.trim());
    let home = format!("/Users/{USER}");
    let grow = grow_by_gib.map_or_else(String::new, |gib| grow_disk(gib * GIB / 10 * 9));
    format!(
        "set -e
IFS= read -r SILO_PASSWORD
IFS= read -r SILO_KCPASSWORD
if [ -z \"$SILO_PASSWORD\" ] || [ -z \"$SILO_KCPASSWORD\" ]; then exit {MISSING_SECRETS}; fi
printf '%s\\n%s\\n' \"$SILO_PASSWORD\" \"$SILO_PASSWORD\" | /usr/bin/dscl . -passwd /Users/{USER}
printf '%s' \"$SILO_KCPASSWORD\" | /usr/bin/base64 -D > /etc/kcpassword.new
/usr/sbin/chown root:wheel /etc/kcpassword.new
/bin/chmod 600 /etc/kcpassword.new
/bin/mv -f /etc/kcpassword.new /etc/kcpassword
unset SILO_PASSWORD SILO_KCPASSWORD
/usr/sbin/scutil --set ComputerName {name}
/usr/sbin/scutil --set LocalHostName {name}
/usr/sbin/scutil --set HostName {name}
/bin/rm -f /etc/ssh/ssh_host_*
/usr/bin/ssh-keygen -A
/usr/bin/find {home}/Library/Keychains -mindepth 1 -maxdepth 1 -exec /bin/rm -rf {{}} + 2>/dev/null || true
{grow}/bin/mkdir -p {home}/.ssh
printf '%s\\n' {authorized_key} > {home}/.ssh/authorized_keys.new
/usr/sbin/chown {UID}:{GID} {home}/.ssh {home}/.ssh/authorized_keys.new
/bin/chmod 700 {home}/.ssh
/bin/chmod 600 {home}/.ssh/authorized_keys.new
/bin/mv -f {home}/.ssh/authorized_keys.new {home}/.ssh/authorized_keys
/bin/sync
/bin/echo {DONE}
"
    )
}

/// Expands the APFS container into the larger disk. It counts as done only when the resize
/// command succeeded and the container grew by at least `minimum_growth` bytes. A failure
/// is reported, not fatal here: the computer works with the template's space.
fn grow_disk(minimum_growth: u64) -> String {
    format!(
        "store=$(/usr/sbin/diskutil info -plist / | /usr/bin/plutil -extract APFSPhysicalStores.0.APFSPhysicalStore raw -o - - 2>/dev/null || true)
whole=$(printf '%s' \"$store\" | /usr/bin/sed 's/s[0-9]*$//')
resized=no
if [ -n \"$store\" ] && [ -n \"$whole\" ]; then
before=$(/usr/sbin/diskutil info -plist \"$store\" | /usr/bin/plutil -extract TotalSize raw -o - - 2>/dev/null || /bin/echo 0)
/bin/echo y | /usr/sbin/diskutil repairDisk \"$whole\" >/dev/null 2>&1 || true
if /usr/sbin/diskutil apfs resizeContainer \"$store\" 0 >/dev/null 2>&1; then
after=$(/usr/sbin/diskutil info -plist \"$store\" | /usr/bin/plutil -extract TotalSize raw -o - - 2>/dev/null || /bin/echo 0)
if [ \"$((after - before))\" -ge {minimum_growth} ] 2>/dev/null; then resized=yes; fi
fi
fi
if [ \"$resized\" = yes ]; then /bin/echo {RESIZED}; else /bin/echo {NOT_RESIZED}; fi
"
    )
}

/// The standard input of the personalization command.
pub(super) fn input(password: &str) -> Vec<u8> {
    let kcpassword = base64_encode(&offline_setup::kcpassword(password));
    format!("{password}\n{kcpassword}\n").into_bytes()
}

/// The command that runs `script` as root with the key of the account. The script is
/// passed as one argument, base64 encoded so no quoting survives to be broken, and the
/// standard input stays free for the secrets.
pub(super) fn command(script: &str) -> String {
    let script = base64_encode(script.as_bytes());
    format!("/usr/bin/sudo -n /bin/sh -c \"$(/bin/echo {script} | /usr/bin/base64 -D)\"")
}

/// Whether the script ran to its end, and how the disk expansion went.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Outcome {
    pub disk_resized: Option<bool>,
}

pub(super) fn outcome(status: i32, stdout: &str) -> Result<Outcome, String> {
    if status == MISSING_SECRETS {
        return Err("Silo could not give the computer its own password.".into());
    }
    if status != 0 || !stdout.lines().any(|line| line.trim() == DONE) {
        return Err("Silo could not personalize the computer.".into());
    }
    let has = |marker: &str| stdout.lines().any(|line| line.trim() == marker);
    Ok(Outcome {
        disk_resized: if has(RESIZED) {
            Some(true)
        } else if has(NOT_RESIZED) {
            Some(false)
        } else {
            None
        },
    })
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    const PASSWORD: &str = "Zx3kQ9mTt2pLw8Yh4nBvC6Rs";
    const KEY: &str = "ssh-ed25519 AAAAC3Nza silo";

    fn decoded(command: &str) -> String {
        let encoded = command
            .split("/bin/echo ")
            .nth(1)
            .and_then(|rest| rest.split(' ').next())
            .unwrap();
        String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn the_password_is_never_in_the_script_or_the_command() {
        let script = script(KEY, "mac-one", Some(128));
        let command = command(&script);
        for text in [&script, &command, &decoded(&command)] {
            assert!(!text.contains(PASSWORD));
        }
        // The password reaches only a pipe into dscl, which reads it from standard input.
        assert!(script.contains("IFS= read -r SILO_PASSWORD"));
        assert!(script.contains("| /usr/bin/dscl . -passwd /Users/silo\n"));
        assert!(!script.contains("-passwd /Users/silo \""));
        assert!(!script.contains("sysadminctl"));
        // Only builtins touch the secrets: no external program names them in its arguments.
        for line in script.lines().filter(|line| line.contains("$SILO_")) {
            assert!(
                !line.contains("/usr/bin/printf") && !line.contains("/bin/echo"),
                "{line}"
            );
        }
        assert!(script.contains("| /usr/bin/base64 -D > /etc/kcpassword.new"));
    }

    #[test]
    fn the_secrets_travel_on_standard_input() {
        let input = String::from_utf8(input(PASSWORD)).unwrap();
        let lines: Vec<_> = input.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], PASSWORD);
        let kcpassword = base64::engine::general_purpose::STANDARD
            .decode(lines[1])
            .unwrap();
        assert_eq!(kcpassword, offline_setup::kcpassword(PASSWORD));
        assert_eq!(kcpassword.len() % 12, 0);
        assert!(input.ends_with('\n'));
    }

    #[test]
    fn the_command_survives_the_shell_round_trip() {
        let script = script("ssh-ed25519 AAAA 'quoted' comment", "mac-one", None);
        let command = command(&script);
        assert_eq!(decoded(&command), script);
        assert!(command.starts_with("/usr/bin/sudo -n /bin/sh -c \"$("));
        assert!(!command.contains('\''));
    }

    #[test]
    fn the_copy_gets_its_own_identity() {
        let script = script(KEY, "mac-one", None);
        for expected in [
            "/etc/kcpassword",
            "scutil --set ComputerName 'mac-one'",
            "scutil --set LocalHostName 'mac-one'",
            "scutil --set HostName 'mac-one'",
            "rm -f /etc/ssh/ssh_host_*",
            "ssh-keygen -A",
            "/Users/silo/Library/Keychains -mindepth 1 -maxdepth 1 -exec /bin/rm -rf {} +",
        ] {
            assert!(script.contains(expected), "{expected}");
        }
        assert!(script.contains(&format!(
            "printf '%s\\n' '{KEY}' > /Users/silo/.ssh/authorized_keys.new"
        )));
    }

    #[test]
    fn the_template_key_is_replaced_last_so_a_failed_run_can_be_repeated() {
        let script = script(KEY, "mac-one", Some(64));
        let at = |needle: &str| script.find(needle).unwrap();
        let replaced = at("mv -f /Users/silo/.ssh/authorized_keys.new");
        for earlier in [
            "dscl",
            "kcpassword",
            "scutil",
            "ssh-keygen",
            "Keychains",
            "resizeContainer",
        ] {
            assert!(at(earlier) < replaced, "{earlier}");
        }
        assert!(script[replaced..].matches("mv -f").count() == 1);
        assert!(script.trim_end().ends_with(DONE));
    }

    #[test]
    fn the_container_is_grown_only_for_a_larger_disk() {
        assert!(!script(KEY, "mac-one", None).contains("resizeContainer"));
        let grown = script(KEY, "mac-one", Some(128));
        assert!(grown.contains("diskutil apfs resizeContainer \"$store\" 0"));
        assert!(grown.contains(&format!("-ge {}", 128 * GIB / 10 * 9)));
        // The command's own status decides, and growth is measured against the container before.
        assert!(grown.contains("if /usr/sbin/diskutil apfs resizeContainer"));
        assert!(grown.contains("before=$(") && grown.contains("after - before"));
    }

    #[test]
    fn the_outcome_reads_the_markers() {
        let ok = format!("{RESIZED}\n{DONE}\n");
        assert_eq!(outcome(0, &ok).unwrap().disk_resized, Some(true));
        let short = format!("{NOT_RESIZED}\n{DONE}\n");
        assert_eq!(outcome(0, &short).unwrap().disk_resized, Some(false));
        assert_eq!(outcome(0, &format!("{DONE}\n")).unwrap().disk_resized, None);
        assert!(outcome(1, &ok).is_err());
        assert!(outcome(0, "").is_err());
        assert!(outcome(MISSING_SECRETS, "")
            .unwrap_err()
            .contains("password"));
    }
}
