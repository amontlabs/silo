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
const UNALLOCATED: &str = "SILO_UNALLOCATED=";
pub(super) const DONE: &str = "SILO_PERSONALIZED";
const MISSING_SECRETS: i32 = 64;

/// The commands run as root. `grow` adds the expansion of the APFS container into a disk
/// that was made larger than the template's. The secrets are only ever read into
/// shell variables and written through the shell's builtin `printf`, so no external process
/// receives them as an argument.
pub(super) fn script(public_key: &str, computer_name: &str, grow: bool) -> String {
    let name = shell_quote(computer_name);
    let authorized_key = shell_quote(public_key.trim());
    let home = format!("/Users/{USER}");
    let grow = if grow { grow_script() } else { String::new() };
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

/// Measures how much of the disk no partition covers, expands the APFS container when that is
/// a gibibyte or more, and measures again. It prints `SILO_UNALLOCATED=<bytes>` (or
/// `unknown`), so it can run on every attempt: it expands only what is still unexpanded, and
/// the result is a measurement of the guest, not of this attempt.
pub(super) fn grow_script() -> String {
    r#"set +e
store=$(/usr/sbin/diskutil info -plist / | /usr/bin/plutil -extract APFSPhysicalStores.0.APFSPhysicalStore raw -o - - 2>/dev/null)
whole=$(printf '%s' "$store" | /usr/bin/sed 's/s[0-9]*$//')
unallocated_bytes() {
list=$(/usr/sbin/diskutil list -plist "$whole" 2>/dev/null) || return 1
total=$(printf '%s' "$list" | /usr/bin/plutil -extract AllDisksAndPartitions.0.Size raw -o - - 2>/dev/null) || return 1
sum=0
n=0
while size=$(printf '%s' "$list" | /usr/bin/plutil -extract "AllDisksAndPartitions.0.Partitions.$n.Size" raw -o - - 2>/dev/null); do
sum=$((sum + size))
n=$((n + 1))
done
/bin/echo $((total - sum))
}
gap=
if [ -n "$store" ] && [ -n "$whole" ]; then
gap=$(unallocated_bytes)
if [ -n "$gap" ] && [ "$gap" -ge GIB_BYTES ]; then
/bin/echo y | /usr/sbin/diskutil repairDisk "$whole" >/dev/null 2>&1
/usr/sbin/diskutil apfs resizeContainer "$store" 0 >/dev/null 2>&1
gap=$(unallocated_bytes)
fi
fi
if [ -n "$gap" ]; then /bin/echo SILO_UNALLOCATED=$gap; else /bin/echo SILO_UNALLOCATED=unknown; fi
set -e
"#
    .replace("GIB_BYTES", &GIB.to_string())
}

/// The disk size a computer really has once the guest has been measured.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Disk {
    pub gib: u64,
    /// The container does not fill the disk that was asked for.
    pub short: bool,
}

/// What `requested_gib` of disk came to, given the bytes no partition covers. A disk smaller
/// than the template's does not exist, and an unmeasurable one counts as not expanded.
pub(super) fn reconcile_disk(
    requested_gib: u64,
    template_gib: u64,
    unallocated: Option<u64>,
) -> Disk {
    match unallocated {
        Some(bytes) if bytes < GIB => Disk {
            gib: requested_gib,
            short: false,
        },
        Some(bytes) => Disk {
            gib: requested_gib
                .saturating_sub(bytes.div_ceil(GIB))
                .clamp(template_gib, requested_gib.max(template_gib)),
            short: true,
        },
        None => Disk {
            gib: template_gib,
            short: true,
        },
    }
}

/// The disk size asked for, as the backing file has it: a copy's file is made that long when
/// it is cloned and nothing changes it, so every attempt reconciles against the same target.
pub(super) fn requested_gib(disk: &std::path::Path) -> Result<u64, String> {
    std::fs::metadata(disk)
        .map(|meta| meta.len() / GIB)
        .map_err(|error| super::store::io_error("read the disk's size", &error))
}

/// The measurement a script printed.
pub(super) fn unallocated(stdout: &str) -> Option<u64> {
    stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix(UNALLOCATED))
        .and_then(|value| value.parse().ok())
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

/// Whether the script ran to its end, and what it measured of the disk.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Outcome {
    pub unallocated: Option<u64>,
}

pub(super) fn outcome(status: i32, stdout: &str) -> Result<Outcome, String> {
    if status == MISSING_SECRETS {
        return Err("Silo could not give the computer its own password.".into());
    }
    if status != 0 || !stdout.lines().any(|line| line.trim() == DONE) {
        return Err("Silo could not personalize the computer.".into());
    }
    Ok(Outcome {
        unallocated: unallocated(stdout),
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
        let script = script(KEY, "mac-one", true);
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
        let script = script("ssh-ed25519 AAAA 'quoted' comment", "mac-one", false);
        let command = command(&script);
        assert_eq!(decoded(&command), script);
        assert!(command.starts_with("/usr/bin/sudo -n /bin/sh -c \"$("));
        assert!(!command.contains('\''));
    }

    #[test]
    fn the_copy_gets_its_own_identity() {
        let script = script(KEY, "mac-one", false);
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
        let script = script(KEY, "mac-one", true);
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
        assert!(!script(KEY, "mac-one", false).contains("resizeContainer"));
        let grown = script(KEY, "mac-one", true);
        assert!(grown.contains("diskutil apfs resizeContainer \"$store\" 0"));
        assert!(grown.contains(&format!("-ge {GIB} ]")));
    }

    #[test]
    fn growth_is_measured_on_every_attempt_and_only_done_while_space_is_unused() {
        let grow = grow_script();
        // The expansion is behind a measurement, which is taken again afterwards.
        let first = grow.find("gap=$(unallocated_bytes)").unwrap();
        let resize = grow.find("resizeContainer").unwrap();
        let second = grow.rfind("gap=$(unallocated_bytes)").unwrap();
        assert!(first < resize && resize < second);
        assert!(grow.contains("SILO_UNALLOCATED=$gap"));
        assert!(grow.contains("SILO_UNALLOCATED=unknown"));
        assert!(!grow.contains("GIB_BYTES"));
    }

    #[test]
    fn the_disk_is_reconciled_with_what_the_guest_measured() {
        let disk = |unallocated| reconcile_disk(128, 64, unallocated);
        let full = Disk {
            gib: 128,
            short: false,
        };
        assert_eq!(disk(Some(0)), full);
        assert_eq!(disk(Some(GIB - 1)), full);
        // 60 GiB unused: the container still has about the template's space.
        assert_eq!(
            disk(Some(60 * GIB)),
            Disk {
                gib: 68,
                short: true
            }
        );
        assert_eq!(
            disk(Some(64 * GIB)),
            Disk {
                gib: 64,
                short: true
            }
        );
        assert_eq!(
            disk(Some(100 * GIB)),
            Disk {
                gib: 64,
                short: true
            }
        );
        assert_eq!(
            disk(None),
            Disk {
                gib: 64,
                short: true
            }
        );
    }

    #[test]
    fn a_retry_reconciles_against_the_same_target_until_verified() {
        let dir = tempfile::tempdir().unwrap();
        let disk = dir.path().join("disk.img");
        std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&disk)
            .and_then(|file| file.set_len(128 * GIB))
            .unwrap();
        // 128 GiB asked for, 60 GiB left unexpanded; verification then fails, so the
        // record keeps its value and the file is untouched.
        let first = reconcile_disk(requested_gib(&disk).unwrap(), 64, Some(60 * GIB));
        assert_eq!(
            first,
            Disk {
                gib: 68,
                short: true
            }
        );
        assert_eq!(requested_gib(&disk).unwrap(), 128);
        // A Retry without a successful expansion reaches the same answer.
        let without = reconcile_disk(requested_gib(&disk).unwrap(), 64, Some(60 * GIB));
        assert_eq!(without, first);
        // A Retry whose expansion worked reaches the full size, not the earlier reduction.
        let with = reconcile_disk(requested_gib(&disk).unwrap(), 64, Some(0));
        assert_eq!(
            with,
            Disk {
                gib: 128,
                short: false
            }
        );
        // An unreadable measurement is not mistaken for a smaller target either.
        assert_eq!(
            reconcile_disk(requested_gib(&disk).unwrap(), 64, None),
            Disk {
                gib: 64,
                short: true
            }
        );
        assert_eq!(requested_gib(&disk).unwrap(), 128);
    }

    #[test]
    fn the_outcome_reads_the_markers() {
        let ok = format!("SILO_UNALLOCATED=4096\n{DONE}\n");
        assert_eq!(outcome(0, &ok).unwrap().unallocated, Some(4096));
        let unknown = format!("SILO_UNALLOCATED=unknown\n{DONE}\n");
        assert_eq!(outcome(0, &unknown).unwrap().unallocated, None);
        assert_eq!(outcome(0, &format!("{DONE}\n")).unwrap().unallocated, None);
        assert_eq!(unallocated("noise\nSILO_UNALLOCATED=7\n"), Some(7));
        assert!(outcome(1, &ok).is_err());
        assert!(outcome(0, "").is_err());
        assert!(outcome(MISSING_SECRETS, "")
            .unwrap_err()
            .contains("password"));
    }
}
