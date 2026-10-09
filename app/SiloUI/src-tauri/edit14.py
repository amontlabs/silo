import os
os.chdir('/Users/polarzero/code/projects/microsandbox-workspaces/.claude/worktrees/agent-ac2c13e3903df9a00/app/SiloUI/src-tauri/src/macos_computers')

def edit(path, pairs):
    s = open(path).read()
    for a, b in pairs:
        assert a in s, (path, a)
        s = s.replace(a, b, 1)
    open(path, 'w').write(s)

edit('offline_setup.rs', [
    ("/usr/sbin/chown root:wheel /var/db/dslocal/nodes/Default/users/{USER}.plist || true", "/usr/sbin/chown root:wheel /var/db/dslocal/nodes/Default/users/{USER}.plist 2>/dev/null || true"),
    ("/Library/Preferences/com.apple.PowerManagement.plist || true", "/Library/Preferences/com.apple.PowerManagement.plist 2>/dev/null || true"),
    ("/var/db/com.apple.xpc.launchd/disabled.migrated || true", "/var/db/com.apple.xpc.launchd/disabled.migrated 2>/dev/null || true"),
    ("/Users/{USER}/Library/Preferences/*.plist || true", "/Users/{USER}/Library/Preferences/*.plist 2>/dev/null || true"),
    ("/usr/sbin/visudo -cf /etc/sudoers.d/{USER}.new\n", "/usr/sbin/visudo -cf /etc/sudoers.d/{USER}.new >/dev/null\n"),
    ("/usr/sbin/diskutil apfs updatePreboot / >/dev/null\n", '''attempt=0
until err=$(/usr/sbin/diskutil apfs updatePreboot / 2>&1) || err=$(/usr/sbin/diskutil apfs updatePreboot /System/Volumes/Data 2>&1); do
attempt=$((attempt + 1))
if [ \\"$attempt\\" -ge 5 ]; then echo \\"$err\\" >&2; exit {PREBOOT_FAILED}; fi
/bin/sleep 5
done
'''),
    ("pub(super) fn finalization_script(public_key: &str) -> String {", '''pub(super) fn finalization_script(public_key: &str) -> String {'''),
    ("const UID: &str = \"501\";", "/// The exit status of the finalization script when the Preboot volume could not be updated.\npub(super) const PREBOOT_FAILED: i32 = 70;\nconst UID: &str = \"501\";"),
])

edit('provision.rs', [
    ('''    if output.status != 0 || !output.stdout.contains("MARKER_OWNER=0:0") {
        return Err(format!(
            "Silo could not finish setting up the account in the computer: {} {}",
            output.stdout.trim(),
            output.stderr.trim()
        ));
    }
    let output = guest_access::run(layout, record, "/usr/bin/sudo -n true", None, GUEST_COMMAND)?;
    if output.status != 0 {
        return Err(format!(
            "Silo cannot log in to the computer with its key and run administrator commands: {}",
            output.stderr.trim()
        ));
    }
    Ok(())
}''', '''    if output.status != 0 || !output.stdout.contains("MARKER_OWNER=0:0") {
        eprintln!(
            "macOS computer setup: the account script failed with status {}.\\nstdout: {}\\nstderr: {}",
            output.status,
            output.stdout.trim(),
            output.stderr.trim()
        );
        return Err(finalization_failure(output.status));
    }
    let output = guest_access::run(layout, record, "/usr/bin/sudo -n true", None, GUEST_COMMAND)?;
    if output.status != 0 {
        eprintln!(
            "macOS computer setup: key login or sudo -n failed with status {}: {}",
            output.status,
            output.stderr.trim()
        );
        return Err(
            "Silo could not log in to the computer with its key and run administrator commands."
                .into(),
        );
    }
    Ok(())
}

/// A one-sentence reason for a failed account script; the details go to the log.
fn finalization_failure(status: i32) -> String {
    if status == offline_setup::PREBOOT_FAILED {
        "Silo could not finish setting up the account: diskutil could not update the preboot volume.".into()
    } else {
        "Silo could not finish setting up the account in the computer.".into()
    }
}'''),
    ('''    #[test]
    fn csrutil_output_decides''', '''    #[test]
    fn a_failed_account_script_gives_one_sentence_without_raw_output() {
        assert_eq!(
            finalization_failure(offline_setup::PREBOOT_FAILED),
            "Silo could not finish setting up the account: diskutil could not update the preboot volume."
        );
        let other = finalization_failure(1);
        assert!(other.ends_with('.') && !other.contains('\\n') && !other.contains("chown"));
    }

    #[test]
    fn csrutil_output_decides'''),
])

edit('offline_setup.rs', [
    ('''        assert!(script.contains("diskutil apfs updatePreboot /"));''', '''        assert!(script.contains("diskutil apfs updatePreboot / 2>&1"));
        assert!(script.contains("updatePreboot /System/Volumes/Data"));
        assert!(script.contains("exit 70;"));
        // Tolerated failures stay out of the script's stderr, which becomes the error.
        for line in script.lines().filter(|line| line.contains("|| true")) {
            assert!(line.contains("2>/dev/null"), "{line}");
        }
        assert!(script.contains("visudo -cf /etc/sudoers.d/silo.new >/dev/null"));'''),
])

edit('engine.rs', [
    ('''pub(super) fn start(app: &AppHandle, record: &Record, layout: &Layout) -> Result<(), String> {
    let model''', '''/// How often a start is retried while the framework still holds the previous
/// machine's lock on the computer's auxiliary storage, and how long it waits between tries.
const LOCK_RETRIES: u32 = 6;
const LOCK_BACKOFF: Duration = Duration::from_secs(2);

pub(super) fn start(app: &AppHandle, record: &Record, layout: &Layout) -> Result<(), String> {
    retry_while_locked(LOCK_RETRIES, LOCK_BACKOFF, || {
        start_once(app, record, layout)
    })
}

/// A machine that just ended (an installation, a stopped computer) releases its
/// auxiliary storage a moment after its object is dropped; the next start fails
/// with a lock error until then.
fn retry_while_locked(
    retries: u32,
    backoff: Duration,
    mut attempt: impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    let mut tries = 0;
    loop {
        match attempt() {
            Err(message) if tries < retries && is_lock_error(&message) => {
                tries += 1;
                std::thread::sleep(backoff);
            }
            other => return other,
        }
    }
}

fn is_lock_error(message: &str) -> bool {
    message.to_ascii_lowercase().contains("lock")
}

fn start_once(app: &AppHandle, record: &Record, layout: &Layout) -> Result<(), String> {
    let model'''),
    ('''    #[test]
    fn host_limits_describe_this_mac''', '''    #[test]
    fn a_start_is_retried_while_the_auxiliary_storage_is_locked() {
        let lock = "Invalid virtual machine configuration. Failed to lock auxiliary storage.";
        let mut calls = 0;
        let result = retry_while_locked(3, Duration::ZERO, || {
            calls += 1;
            if calls < 3 {
                Err(lock.to_string())
            } else {
                Ok(())
            }
        });
        assert_eq!(result, Ok(()));
        assert_eq!(calls, 3);

        // The retries are bounded, and the last error is reported.
        let mut calls = 0;
        let result = retry_while_locked(2, Duration::ZERO, || {
            calls += 1;
            Err(lock.to_string())
        });
        assert_eq!(result, Err(lock.to_string()));
        assert_eq!(calls, 3);

        // Any other failure is reported at once.
        let mut calls = 0;
        let result = retry_while_locked(5, Duration::ZERO, || {
            calls += 1;
            Err("macOS allows at most two".to_string())
        });
        assert!(result.is_err());
        assert_eq!(calls, 1);
    }

    #[test]
    fn host_limits_describe_this_mac'''),
])
