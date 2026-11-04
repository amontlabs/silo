//! The first start of a fork that was personalized while stopped.
//!
//! Its disk already has the new password and automatic login, no login keychain, no SSH
//! keys of the source, and the network configuration of its own gateway address, so it
//! boots once, behind its gateway. This module runs in that boot, before the computer is
//! handed over: it logs in with the new password, installs the fork's key, name and host
//! keys, the network files and the trust of the fork's proxy CA, and gives the account its
//! SecureToken back.
use super::{
    checkpoints, guest_access, offline_setup, personalize, proxy, store, update, MacosContext,
};
use std::time::Duration;

const SSH_WAIT: Duration = Duration::from_secs(300);
const COMMAND: Duration = Duration::from_secs(120);

/// Finishes the personalization of a fork in its first boot and saves the result.
pub(super) fn complete(
    app: &MacosContext,
    record: &store::Record,
    layout: &store::Layout,
) -> Result<(), String> {
    let account = guest_access::account(layout)?;
    guest_access::reset_host_keys(layout)?;
    guest_access::wait_for_password_ssh(layout, record, SSH_WAIT, &|| false)?;
    let public_key = guest_access::public_key(&account)?;
    let command = identity_command(&public_key, &record.name);
    let password = format!("{}\n", account.password);
    let output = guest_access::run_with_password(
        layout,
        record,
        &command,
        Some(password.as_bytes()),
        COMMAND,
    )?;
    if output.status != 0 || !output.stdout.contains("MARKER_OWNER=0:0") {
        return Err("Silo could not finish setting up the fork's account.".into());
    }
    // The script replaced the host keys, and every new connection presents them.
    guest_access::reset_host_keys(layout)?;
    guest_access::wait_for_ssh(layout, record, SSH_WAIT, &|| false)?;
    proxy::install_on_running_guest(app, record, layout)?;
    let token = guest_access::run(
        layout,
        record,
        &personalize::command(&personalize::secure_token_script()),
        Some(&personalize::secure_token_input(&account.password)),
        COMMAND,
    )
    .map(|output| personalize::secure_token_outcome(output.status, &output.stdout))
    .unwrap_or(personalize::SecureToken::Unknown);
    let mut done = record.clone();
    done.first_boot = false;
    done.inherited_access = false;
    done.secure_token_missing = token != personalize::SecureToken::Enabled;
    store::save(layout, &done).map_err(|error| format!("{error:?}"))?;
    update(app, &done.id, |entry| entry.record = done.clone());
    // Its own credentials are the only ones left.
    let _ = std::fs::remove_dir_all(checkpoints::inherited_access(layout));
    Ok(())
}

/// The root command over the password login: the fork's key, name and host keys.
fn identity_command(public_key: &str, computer_name: &str) -> String {
    use base64::Engine;
    let script = base64::engine::general_purpose::STANDARD
        .encode(offline_setup::fork_finalization_script(public_key, computer_name));
    format!("/usr/bin/sudo -S -p '' /bin/sh -c 'echo {script} | /usr/bin/base64 -D | /bin/sh'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_identity_command_carries_the_script_as_one_quoted_argument() {
        let command = identity_command("ssh-ed25519 AAAA fork", "my fork");
        assert!(command.starts_with("/usr/bin/sudo -S -p ''"));
        assert_eq!(command.matches('\'').count(), 4);
        assert!(!command.contains("my fork"));
    }
}
