//! The first start of a fork, or of an import, that was personalized while stopped.
//!
//! Its disk already has the new password and automatic login, no login keychain, no SSH
//! keys of the source, and the network configuration of its own gateway address. Its
//! scheduled work was moved aside on the stopped disk (see `quiet`), so it boots once, behind
//! its gateway, with none of it. This module runs in that boot, before the computer is
//! handed over: it logs in with the new password, stops the processes and launchd labels the
//! fork was asked to quiet, installs the fork's key, name and host keys, the network files
//! and the trust of the fork's proxy CA, and gives the account its SecureToken back.
use super::{
    guest_access::{self, CommandOutput},
    offline_setup, personalize, proxy, quiet,
    setup_log::SetupLog,
    store, update, MacosContext,
};
use std::time::Duration;

const SSH_WAIT: Duration = Duration::from_secs(300);
const COMMAND: Duration = Duration::from_secs(120);

/// What the first boot found out, to be saved on the computer's current record.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Outcome {
    pub secure_token_missing: bool,
}

/// The guest as the first boot reaches it.
pub(super) trait Guest {
    fn reset_host_keys(&self) -> Result<(), String>;
    fn wait_for_password_login(&self, cancelled: &dyn Fn() -> bool) -> Result<(), String>;
    fn wait_for_key_login(&self, cancelled: &dyn Fn() -> bool) -> Result<(), String>;
    /// Runs a script as root over the password login.
    fn run_script(&self, script: &str) -> Result<CommandOutput, String>;
    fn install_trust(&self) -> Result<(), String>;
    /// Whether the account has no SecureToken after the offline password change.
    fn secure_token_missing(&self) -> bool;
    fn log_command(&self, what: &str, output: &CommandOutput);
}

/// Finishes the personalization of a fork in its first boot. The steps run in order and the
/// first failure ends it, leaving the fork waiting for another Start.
pub(super) fn finish(
    guest: &dyn Guest,
    stop: &[String],
    identity_script: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Outcome, String> {
    guest.reset_host_keys()?;
    guest.wait_for_password_login(cancelled)?;
    // Before anything else in the guest: what the fork was asked to quiet.
    if let Some(script) = quiet::runtime_script(stop) {
        let output = guest.run_script(&script)?;
        if output.status != 0 {
            guest.log_command("quiet the fork", &output);
            return Err("Silo could not stop the processes named for the fork.".into());
        }
    }
    let output = guest.run_script(identity_script)?;
    if output.status != 0 || !output.stdout.contains("MARKER_OWNER=0:0") {
        guest.log_command("account script", &output);
        return Err("Silo could not finish setting up the computer's account.".into());
    }
    // The script replaced the host keys, and every new connection presents them.
    guest.reset_host_keys()?;
    guest.wait_for_key_login(cancelled)?;
    guest.install_trust()?;
    Ok(Outcome {
        secure_token_missing: guest.secure_token_missing(),
    })
}

/// Runs the first boot and, only when it succeeded, saves its result. `save` receives the
/// outcome and returns the record as saved.
pub(super) fn complete_with(
    guest: &dyn Guest,
    stop: &[String],
    identity_script: &str,
    cancelled: &dyn Fn() -> bool,
    save: &mut dyn FnMut(&Outcome) -> Result<store::Record, String>,
) -> Result<store::Record, String> {
    let outcome = finish(guest, stop, identity_script, cancelled)?;
    save(&outcome)
}

/// Applies the first boot's result to a record. Nothing else on the record changes.
pub(super) fn apply_outcome(record: &mut store::Record, outcome: &Outcome) {
    record.first_boot = false;
    record.inherited_access = false;
    // The guest's receipt is the source's. The next update writes the fork's own, and the
    // computer is not reported ready before it.
    record.computer_use_version = None;
    record.secure_token_missing = outcome.secure_token_missing;
    if let Some(quiet) = record.quiet.as_mut() {
        quiet.applied = true;
    }
}

/// Saves the outcome on a copy of the record and only then makes it the current one, so a
/// failed save leaves the record saying the first boot is still to come.
fn commit_outcome(
    current: &mut store::Record,
    outcome: &Outcome,
    save: &dyn Fn(&store::Record) -> Result<(), String>,
) -> Result<store::Record, String> {
    let mut next = current.clone();
    apply_outcome(&mut next, outcome);
    save(&next)?;
    *current = next.clone();
    Ok(next)
}

/// Finishes the personalization of a fork in its first boot and saves the result on the
/// computer's current record, which other operations may have changed since the Start began.
/// Returns that record.
pub(super) fn complete(
    app: &MacosContext,
    record: &store::Record,
    layout: &store::Layout,
    cancelled: &dyn Fn() -> bool,
) -> Result<store::Record, String> {
    let account = guest_access::account(layout)?;
    let public_key = guest_access::public_key(&account)?;
    let identity_script = identity_script(record, &public_key);
    let log = SetupLog::open(app, &record.id);
    if let Some(log) = &log {
        log.hide(&account.password);
    }
    let guest = Live {
        app,
        record,
        layout,
        account,
        log,
    };
    let stop = record
        .quiet
        .as_ref()
        .map(|quiet| quiet.stop.clone())
        .unwrap_or_default();
    let id = record.id.clone();
    complete_with(&guest, &stop, &identity_script, cancelled, &mut |outcome| {
        let saved = update(app, &id, |entry| {
            commit_outcome(&mut entry.record, outcome, &|record| {
                store::save(layout, record)
            })
        })
        .ok_or_else(|| "This computer no longer exists.".to_string())
        .and_then(|result| result)?;
        // Its own credentials are the only ones left.
        let _ = std::fs::remove_dir_all(super::checkpoints::inherited_access(layout));
        Ok(saved)
    })
}

struct Live<'a> {
    app: &'a MacosContext,
    record: &'a store::Record,
    layout: &'a store::Layout,
    account: guest_access::GuestAccount,
    log: Option<SetupLog>,
}

impl Guest for Live<'_> {
    fn reset_host_keys(&self) -> Result<(), String> {
        guest_access::reset_host_keys(self.layout)
    }

    fn wait_for_password_login(&self, cancelled: &dyn Fn() -> bool) -> Result<(), String> {
        guest_access::wait_for_password_ssh(self.layout, self.record, SSH_WAIT, cancelled)
            .map(|_| ())
    }

    fn wait_for_key_login(&self, cancelled: &dyn Fn() -> bool) -> Result<(), String> {
        guest_access::wait_for_ssh(self.layout, self.record, SSH_WAIT, cancelled).map(|_| ())
    }

    fn run_script(&self, script: &str) -> Result<CommandOutput, String> {
        let password = format!("{}\n", self.account.password);
        guest_access::run_with_password(
            self.layout,
            self.record,
            &root_command(script),
            Some(password.as_bytes()),
            COMMAND,
        )
    }

    fn install_trust(&self) -> Result<(), String> {
        proxy::install_on_running_guest(self.app, self.record, self.layout)
    }

    fn secure_token_missing(&self) -> bool {
        let token = guest_access::run(
            self.layout,
            self.record,
            &personalize::command(&personalize::secure_token_script()),
            Some(&personalize::secure_token_input(&self.account.password)),
            COMMAND,
        )
        .map(|output| personalize::secure_token_outcome(output.status, &output.stdout))
        .unwrap_or(personalize::SecureToken::Unknown);
        token != personalize::SecureToken::Enabled
    }

    fn log_command(&self, what: &str, output: &CommandOutput) {
        if let Some(log) = &self.log {
            log.command(what, output.status, &output.stdout, &output.stderr);
        }
    }
}

/// The account script of this boot: an imported disk also has its login keychains cleared.
fn identity_script(record: &store::Record, public_key: &str) -> String {
    if record.imported {
        offline_setup::import_finalization_script(public_key, &record.name)
    } else {
        offline_setup::fork_finalization_script(public_key, &record.name)
    }
}

/// The root command over the password login: the script travels as one quoted argument.
fn root_command(script: &str) -> String {
    use base64::Engine;
    let script = base64::engine::general_purpose::STANDARD.encode(script);
    format!("/usr/bin/sudo -S -p '' /bin/sh -c 'echo {script} | /usr/bin/base64 -D | /bin/sh'")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake {
        calls: RefCell<Vec<String>>,
        scripts: RefCell<Vec<String>>,
        logged: RefCell<Vec<String>>,
        fail_identity: bool,
        fail_quiet: bool,
        fail_trust: bool,
        no_token: bool,
    }

    impl Fake {
        fn note(&self, call: &str) {
            self.calls.borrow_mut().push(call.into());
        }
        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    impl Guest for Fake {
        fn reset_host_keys(&self) -> Result<(), String> {
            self.note("reset keys");
            Ok(())
        }
        fn wait_for_password_login(&self, cancelled: &dyn Fn() -> bool) -> Result<(), String> {
            self.note("wait password");
            if cancelled() {
                return Err(guest_access::CANCELLED.into());
            }
            Ok(())
        }
        fn wait_for_key_login(&self, cancelled: &dyn Fn() -> bool) -> Result<(), String> {
            self.note("wait key");
            if cancelled() {
                return Err(guest_access::CANCELLED.into());
            }
            Ok(())
        }
        fn run_script(&self, script: &str) -> Result<CommandOutput, String> {
            self.scripts.borrow_mut().push(script.into());
            let quiet = script.contains("launchctl");
            self.note(if quiet { "quiet" } else { "identity" });
            let failed = if quiet {
                self.fail_quiet
            } else {
                self.fail_identity
            };
            Ok(CommandOutput {
                status: i32::from(failed),
                stdout: if failed {
                    "partial".into()
                } else {
                    "MARKER_OWNER=0:0".into()
                },
                stderr: "oops".into(),
            })
        }
        fn install_trust(&self) -> Result<(), String> {
            self.note("trust");
            if self.fail_trust {
                Err("trust failed".into())
            } else {
                Ok(())
            }
        }
        fn secure_token_missing(&self) -> bool {
            self.note("token");
            self.no_token
        }
        fn log_command(&self, what: &str, output: &CommandOutput) {
            self.logged
                .borrow_mut()
                .push(format!("{what}: {} {}", output.status, output.stderr));
        }
    }

    fn record() -> store::Record {
        let request = store::CreateRequest {
            name: "fork".into(),
            cpus: 4,
            memory_gib: 8,
            disk_gib: 64,
        };
        let mut record = store::new_record(&request, "02:00:00:00:00:01".into());
        record.first_boot = true;
        record.inherited_access = true;
        record
    }

    #[test]
    fn an_imported_disk_gets_the_import_account_script_and_a_fork_the_fork_script() {
        let fork = record();
        let forked = identity_script(&fork, "ssh-ed25519 AAAA key");
        assert!(!forked.contains("Library/Keychains"));
        let mut imported = record();
        imported.imported = true;
        let script = identity_script(&imported, "ssh-ed25519 AAAA key");
        assert!(script.contains("Library/Keychains"));
        assert!(script.contains("ssh-keygen -A"));
    }

    #[test]
    fn the_identity_command_carries_the_script_as_one_quoted_argument() {
        let command = root_command(&offline_setup::fork_finalization_script(
            "ssh-ed25519 AAAA fork",
            "my fork",
        ));
        assert!(command.starts_with("/usr/bin/sudo -S -p ''"));
        assert_eq!(command.matches('\'').count(), 4);
        assert!(!command.contains("my fork"));
    }

    #[test]
    fn the_named_processes_are_stopped_before_the_identity_script_and_the_rest() {
        let guest = Fake::default();
        let outcome = finish(&guest, &["poller".into()], "identity", &|| false).unwrap();
        assert!(!outcome.secure_token_missing);
        assert_eq!(
            guest.calls(),
            [
                "reset keys",
                "wait password",
                "quiet",
                "identity",
                "reset keys",
                "wait key",
                "trust",
                "token"
            ]
        );
    }

    #[test]
    fn nothing_named_means_no_quiet_command() {
        let guest = Fake::default();
        finish(&guest, &[], "identity", &|| false).unwrap();
        assert!(!guest.calls().contains(&"quiet".to_string()));
    }

    #[test]
    fn stop_interrupts_the_waits_for_the_guest() {
        let guest = Fake::default();
        let result = finish(&guest, &["poller".into()], "identity", &|| true);
        assert_eq!(result, Err(guest_access::CANCELLED.to_string()));
        assert_eq!(guest.calls(), ["reset keys", "wait password"]);
    }

    #[test]
    fn a_failed_script_is_logged_with_its_output_and_ends_the_boot() {
        let guest = Fake {
            fail_identity: true,
            ..Default::default()
        };
        let result = finish(&guest, &[], "identity", &|| false);
        assert!(result.is_err());
        assert_eq!(
            guest.logged.borrow().as_slice(),
            ["account script: 1 oops"]
        );
        let guest = Fake {
            fail_quiet: true,
            ..Default::default()
        };
        assert!(finish(&guest, &["poller".into()], "identity", &|| false).is_err());
        assert_eq!(guest.logged.borrow().as_slice(), ["quiet the fork: 1 oops"]);
        // The identity script never ran after a failed quiet step.
        assert!(!guest.calls().contains(&"identity".to_string()));
    }

    #[test]
    fn a_failed_boot_saves_nothing_so_the_next_start_retries() {
        let guest = Fake {
            fail_trust: true,
            ..Default::default()
        };
        let mut saved = 0;
        let result = complete_with(&guest, &[], "identity", &|| false, &mut |_| {
            saved += 1;
            Ok(record())
        });
        assert!(result.is_err());
        assert_eq!(saved, 0);
    }

    #[test]
    fn a_failed_save_leaves_the_first_boot_pending_in_memory() {
        let mut current = record();
        let outcome = Outcome {
            secure_token_missing: true,
        };
        let failed = commit_outcome(&mut current, &outcome, &|_| Err("disk full".into()));
        assert_eq!(failed, Err("disk full".to_string()));
        assert!(current.first_boot && current.inherited_access && !current.secure_token_missing);
        let saved = commit_outcome(&mut current, &outcome, &|_| Ok(())).unwrap();
        assert!(!current.first_boot && !saved.first_boot && current.secure_token_missing);
    }

    #[test]
    fn a_finished_boot_changes_only_its_own_fields_on_the_current_record() {
        let guest = Fake {
            no_token: true,
            ..Default::default()
        };
        // The record the Start began with is not what gets saved: the registry's current one is.
        let started_with = record();
        let mut current = record();
        current.disk_gib = 128;
        current.setup_version = Some("changed while the boot ran".into());
        current.quiet = Some(quiet::QuietRecord {
            parked: vec!["/Library/LaunchAgents/a.plist".into()],
            ..Default::default()
        });
        let saved = complete_with(&guest, &[], "identity", &|| false, &mut |outcome| {
            apply_outcome(&mut current, outcome);
            Ok(current.clone())
        })
        .unwrap();
        assert!(!saved.first_boot && !saved.inherited_access && saved.secure_token_missing);
        assert_eq!(saved.disk_gib, 128);
        assert_eq!(
            saved.setup_version.as_deref(),
            Some("changed while the boot ran")
        );
        // The source's receipt is not the fork's: its own update runs after the first Start.
        assert!(saved.computer_use_stale());
        let quiet = saved.quiet.unwrap();
        assert!(quiet.applied);
        assert_eq!(quiet.parked, ["/Library/LaunchAgents/a.plist"]);
        assert_ne!(saved.disk_gib, started_with.disk_gib);
    }
}
