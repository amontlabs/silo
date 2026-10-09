//! Provisioning of an installed macOS computer for computer use: an account with
//! automatic login and SSH, System Integrity Protection off, computer use and the
//! clipboard agent installed. Progress is kept in the computer's record, so a retry
//! resumes at the first unfinished step.
//!
//! The computer's state stays `setting-up` throughout; each step names itself in
//! the detail. Machines started here have no display.
use super::{
    engine, guest_access, guest_clipboard, guest_computer_use, layout_and_record, offline_setup,
    recovery, set_detail,
    store::{self, Layout, Record, SetupProgress},
    Stop, RUN,
};
use std::{
    sync::atomic::{AtomicU8, Ordering},
    time::{Duration, Instant},
};
use tauri::AppHandle;

/// How long the first boot may take to give the guest an address.
const FIRST_BOOT_ADDRESS: Duration = Duration::from_secs(120);
/// Lume lets the first boot settle this long once the guest has an address.
const FIRST_BOOT_SETTLE: Duration = Duration::from_secs(10);
const SSH_WAIT: Duration = Duration::from_secs(300);
const GUEST_COMMAND: Duration = Duration::from_secs(120);
const SHUTDOWN_WAIT: Duration = Duration::from_secs(120);
const FORCED_STOP_WAIT: Duration = Duration::from_secs(30);
const MACHINE_POLL: Duration = Duration::from_millis(500);

/// Runs every unfinished step for `record`, saving progress after each.
pub(super) fn run(
    app: &AppHandle,
    layout: &Layout,
    record: &mut Record,
    cancel: &AtomicU8,
) -> Result<(), Stop> {
    let provision = Provision {
        app,
        layout,
        id: record.id.clone(),
        cancel,
    };
    let result = provision.steps(record);
    if result.is_err() {
        // Nothing may report the setup as over while the machine can still run.
        provision.ensure_stopped();
    }
    result
}

struct Provision<'a> {
    app: &'a AppHandle,
    layout: &'a Layout,
    id: String,
    cancel: &'a AtomicU8,
}

impl Provision<'_> {
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst) != RUN
    }

    fn check(&self) -> Result<(), Stop> {
        if self.cancelled() {
            Err(Stop::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Names the current step, unless the setup is already ending.
    fn say(&self, step: &str) -> Result<(), Stop> {
        self.check()?;
        set_detail(self.app, &self.id, step);
        Ok(())
    }

    /// Records a finished step and saves it.
    fn mark(
        &self,
        record: &mut Record,
        finished: impl FnOnce(&mut SetupProgress),
    ) -> Result<(), Stop> {
        finished(&mut record.setup);
        store::save(self.layout, record)?;
        let saved = record.clone();
        super::update(self.app, &self.id, |entry| entry.record = saved);
        Ok(())
    }

    fn steps(&self, record: &mut Record) -> Result<(), Stop> {
        if !record.setup.account {
            self.create_account(record)?;
        }
        if !record.setup.sip {
            self.say("Turning off System Integrity Protection")?;
            recovery::disable_sip(self.app, &self.id, &|| self.cancelled()).map_err(|message| {
                if self.cancelled() {
                    Stop::Cancelled
                } else {
                    Stop::Failed(message)
                }
            })?;
            self.check()?;
        }
        if record.setup.sip && record.setup.computer_use && record.setup.clipboard {
            return Ok(());
        }
        self.say("Starting macOS")?;
        self.boot(Login::Key)?;
        if !record.setup.sip {
            self.say("Checking System Integrity Protection")?;
            self.verify_sip_disabled()?;
            self.mark(record, |setup| setup.sip = true)?;
        }
        if !record.setup.computer_use {
            self.say("Installing computer use")?;
            guest_computer_use::install(self.app, &self.id)?;
            self.mark(record, |setup| setup.computer_use = true)?;
        }
        if !record.setup.clipboard {
            self.say("Setting up the clipboard")?;
            guest_clipboard::install(self.app, &self.id)?;
            self.mark(record, |setup| setup.clipboard = true)?;
        }
        self.shut_down()
    }

    /// The account, its automatic login and SSH: a first boot so macOS writes its
    /// first-boot state, the offline edit, and a second boot to finish what only
    /// the running guest can.
    fn create_account(&self, record: &mut Record) -> Result<(), Stop> {
        self.say("Starting macOS for the first time")?;
        self.first_boot()?;
        self.say("Creating the account")?;
        let account = guest_access::account(self.layout)?;
        let image = record.restore_image.clone();
        let release = image.as_ref().map(|image| offline_setup::Release {
            version: &image.version,
            build: &image.build,
        });
        offline_setup::run(&self.layout.disk(), &account, release)?;
        self.say("Starting macOS")?;
        self.boot(Login::Password)?;
        self.say("Finishing the account")?;
        self.finalize(&account)?;
        self.shut_down()?;
        self.mark(record, |setup| setup.account = true)
    }

    /// Runs the root script with the account's password, which is the only
    /// credential a new guest has, then checks that the key and `sudo -n` work.
    fn finalize(&self, account: &guest_access::GuestAccount) -> Result<(), Stop> {
        let (layout, record) = layout_and_record(self.app, &self.id)?;
        finalize_guest(&layout, &record, account).map_err(Stop::Failed)
    }

    fn verify_sip_disabled(&self) -> Result<(), Stop> {
        let (layout, record) = layout_and_record(self.app, &self.id)?;
        let output = guest_access::run(
            &layout,
            &record,
            "/usr/bin/csrutil status",
            None,
            GUEST_COMMAND,
        )?;
        if sip_disabled(&output.stdout) {
            Ok(())
        } else {
            Err(Stop::Failed(
                "System Integrity Protection is still on in the computer.".into(),
            ))
        }
    }

    /// Boots the computer without a display and waits for SSH.
    fn boot(&self, login: Login) -> Result<(), Stop> {
        self.check()?;
        let (layout, record) = layout_and_record(self.app, &self.id)?;
        offline_setup::ensure_detached(&layout.disk())?;
        engine::start(self.app, &record, &layout)?;
        let cancelled = || self.cancelled();
        match login {
            Login::Key => guest_access::wait_for_ssh(&layout, &record, SSH_WAIT, &cancelled),
            Login::Password => {
                guest_access::wait_for_password_ssh(&layout, &record, SSH_WAIT, &cancelled)
            }
        }
        .map_err(|message| {
            if self.cancelled() {
                Stop::Cancelled
            } else {
                Stop::Failed(message)
            }
        })?;
        Ok(())
    }

    /// Lets macOS write its first-boot state, then stops it hard, as Lume does.
    fn first_boot(&self) -> Result<(), Stop> {
        self.check()?;
        let (layout, record) = layout_and_record(self.app, &self.id)?;
        offline_setup::ensure_detached(&layout.disk())?;
        engine::start(self.app, &record, &layout)?;
        let deadline = Instant::now() + FIRST_BOOT_ADDRESS;
        while guest_access::guest_address(&record.mac_address).is_err() {
            self.check()?;
            if Instant::now() >= deadline {
                return Err(Stop::Failed(
                    "The computer did not start for the first time.".into(),
                ));
            }
            std::thread::sleep(MACHINE_POLL);
        }
        let settled = Instant::now() + FIRST_BOOT_SETTLE;
        while Instant::now() < settled {
            self.check()?;
            std::thread::sleep(MACHINE_POLL);
        }
        engine::force_stop(self.app, &self.id)?;
        self.wait_stopped(FORCED_STOP_WAIT)
    }

    /// Asks macOS to shut down and waits; a guest that ignores it is stopped hard.
    fn shut_down(&self) -> Result<(), Stop> {
        self.say("Shutting down")?;
        let (layout, record) = layout_and_record(self.app, &self.id)?;
        // The connection ends with the guest, so its outcome says nothing.
        let _ = guest_access::run(
            &layout,
            &record,
            "/usr/bin/sudo -n /sbin/shutdown -h now",
            None,
            GUEST_COMMAND,
        );
        if self.wait_stopped(SHUTDOWN_WAIT).is_err() {
            engine::force_stop(self.app, &self.id)?;
            self.wait_stopped(FORCED_STOP_WAIT)?;
        }
        Ok(())
    }

    /// Waits until the framework holds no machine for this computer.
    fn wait_stopped(&self, timeout: Duration) -> Result<(), Stop> {
        let deadline = Instant::now() + timeout;
        loop {
            let states = engine::machine_states(self.app)?;
            match states.iter().find(|(id, _)| *id == self.id) {
                None => return Ok(()),
                Some((_, engine::MachineState::Stopped | engine::MachineState::Failed)) => {
                    // The framework reported the end, but its delegate has not released the machine.
                    super::machine_stopped(self.app, &self.id, None);
                }
                Some(_) => {}
            }
            if Instant::now() >= deadline {
                return Err(Stop::Failed("The computer did not stop in time.".into()));
            }
            std::thread::sleep(MACHINE_POLL);
        }
    }

    /// Stops the computer's machine after a failed or cancelled setup and returns
    /// only once the framework holds no machine for it. Until then the computer
    /// stays in setup, so it can be neither deleted nor reported as stopped.
    fn ensure_stopped(&self) {
        loop {
            let held = engine::machine_states(self.app)
                .map_or(true, |states| states.iter().any(|(id, _)| *id == self.id));
            if !held {
                return;
            }
            set_detail(self.app, &self.id, "Stopping the computer");
            let _ = engine::force_stop(self.app, &self.id);
            if self.wait_stopped(FORCED_STOP_WAIT).is_ok() {
                return;
            }
            std::thread::sleep(MACHINE_POLL);
        }
    }
}

/// How the first connection to a boot proves who it is.
#[derive(Clone, Copy)]
enum Login {
    /// The account's key, installed by the finalization.
    Key,
    /// The account's password, for a guest whose key is not installed yet.
    Password,
}

/// Finishes the account inside a booted guest that only accepts the password.
fn finalize_guest(
    layout: &Layout,
    record: &Record,
    account: &guest_access::GuestAccount,
) -> Result<(), String> {
    let public_key = guest_access::public_key(account)?;
    let script = base64_encode(offline_setup::finalization_script(&public_key).as_bytes());
    let command =
        format!("/usr/bin/sudo -S -p '' /bin/sh -c 'echo {script} | /usr/bin/base64 -D | /bin/sh'");
    let password = format!("{}\n", account.password);
    let output = guest_access::run_with_password(
        layout,
        record,
        &command,
        Some(password.as_bytes()),
        GUEST_COMMAND,
    )?;
    if output.status != 0 || !output.stdout.contains("MARKER_OWNER=0:0") {
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
}

fn sip_disabled(csrutil_status: &str) -> bool {
    csrutil_status.to_ascii_lowercase().contains("disabled")
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_finalization_script_survives_the_shell_round_trip() {
        use base64::Engine;
        let script = offline_setup::finalization_script("ssh-ed25519 AAAA test");
        let encoded = base64_encode(script.as_bytes());
        assert!(!encoded.contains('\''));
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        assert_eq!(decoded, script.as_bytes());
    }

    #[test]
    fn csrutil_output_decides_whether_protection_is_off() {
        assert!(sip_disabled(
            "System Integrity Protection status: disabled.\n"
        ));
        assert!(!sip_disabled(
            "System Integrity Protection status: enabled.\n"
        ));
    }

    fn live_layout() -> (Layout, Record) {
        let dir = std::path::PathBuf::from(std::env::var("SILO_LIVE_DIR").expect("SILO_LIVE_DIR"));
        let request = store::CreateRequest {
            name: "live".into(),
            cpus: 4,
            memory_gib: 8,
            disk_gib: 64,
        };
        let mac = std::fs::read_to_string(dir.join("mac-address.txt")).unwrap();
        let mut record = store::new_record(&request, mac.trim().into());
        record.id = "live".into();
        (Layout { dir }, record)
    }

    /// Patches the disk of a clone. Run by hand: `SILO_LIVE_DIR=<clone> cargo test live_offline -- --ignored`.
    #[test]
    #[ignore = "needs a clone of an installed computer"]
    fn live_offline() {
        let (layout, _) = live_layout();
        let account = guest_access::account(&layout).unwrap();
        offline_setup::run(
            &layout.disk(),
            &account,
            Some(offline_setup::Release {
                version: "26.6.2",
                build: "25G83",
            }),
        )
        .unwrap();
    }

    /// Finishes the account of a clone that is booted and reachable. Run by hand.
    #[test]
    #[ignore = "needs a running clone"]
    fn live_finalize() {
        let (layout, record) = live_layout();
        let account = guest_access::account(&layout).unwrap();
        let address = guest_access::wait_for_password_ssh(
            &layout,
            &record,
            Duration::from_secs(300),
            &|| false,
        )
        .unwrap();
        println!("password login works at {address}");
        finalize_guest(&layout, &record, &account).unwrap();
        println!("finalized; key login and sudo -n work");
    }
}
