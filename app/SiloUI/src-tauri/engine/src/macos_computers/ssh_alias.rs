//! Stable, ID-keyed SSH aliases for macOS guests. The existing setup key and
//! known-hosts file remain owned by `guest_access`; this module writes client config.
use super::{files, guest_access, MacosContext};
use crate::{applications::launch, editor};
use silo_api::methods::{
    agents::AgentId, guest::GuestSshAliasEnsureResult, terminal::CommandResult,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};

const SSH: &str = "/usr/bin/ssh";
const FAILED: &str = "Could not prepare the SSH connection.";
static LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
static PROXY_HELPER_OVERRIDE: Mutex<Option<PathBuf>> = Mutex::new(None);

fn lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn validate_id(id: &str) -> Result<(), String> {
    uuid::Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| "Invalid computer identity.".to_string())
}

fn alias(id: &str) -> String {
    format!("silo-{id}")
}

fn config_root(app: &MacosContext) -> PathBuf {
    app.paths().app_data.join("ssh/macos")
}

fn config_path(app: &MacosContext, id: &str) -> PathBuf {
    config_root(app).join(format!("{id}.conf"))
}

fn known_hosts(layout: &super::store::Layout) -> PathBuf {
    layout.dir.join("guest-access/known_hosts")
}

fn proxy_helper() -> Result<PathBuf, String> {
    #[cfg(test)]
    if let Some(helper) =
        crate::sync::lock_or_recover(&PROXY_HELPER_OVERRIDE, "SSH proxy helper test override")
            .clone()
    {
        if !launch::executable_file(&helper) {
            return Err("The Silo engine SSH helper is unavailable. Update Silo and retry.".into());
        }
        return Ok(helper);
    }
    let executable = std::env::current_exe().map_err(|_| FAILED)?;
    let helper = executable.parent().ok_or(FAILED)?.join("silod");
    if !launch::executable_file(&helper) {
        return Err("The Silo engine SSH helper is unavailable. Update Silo and retry.".into());
    }
    Ok(helper)
}

#[cfg(test)]
pub(super) fn set_proxy_helper_for_test(helper: Option<PathBuf>) {
    *crate::sync::lock_or_recover(&PROXY_HELPER_OVERRIDE, "SSH proxy helper test override") =
        helper;
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn proxy_text(helper: &str, device_id: &str, computer_id: &str, port: Option<u16>) -> String {
    let mut command = format!(
        "{} --remote-guest {} {}",
        shell_quote(helper),
        shell_quote(device_id),
        shell_quote(computer_id)
    );
    if let Some(port) = port {
        command.push_str(&format!(" {}", shell_quote(&port.to_string())));
    }
    command
}

/// The command `ssh` runs to reach the guest, as the alias's `ProxyCommand` but with a
/// published-port label when `port` is given. The label names the forward on the stream so
/// that unpublishing it ends the stream; the stream still carries SSH to the guest.
pub(super) fn proxy_command(computer_id: &str, port: Option<u16>) -> Result<String, String> {
    validate_id(computer_id)?;
    if port == Some(0) {
        return Err(FAILED.into());
    }
    let device_id = crate::remote::log_identity()?.0;
    validate_id(&device_id)?;
    let helper = proxy_helper()?;
    let helper = helper.to_str().ok_or(FAILED)?;
    if helper.chars().any(char::is_control) {
        return Err(FAILED.into());
    }
    Ok(proxy_text(helper, &device_id, computer_id, port))
}

fn config_text(
    device_id: &str,
    computer_id: &str,
    identity: &Path,
    known_hosts: &Path,
    helper: &Path,
) -> Result<String, String> {
    config_text_for(
        device_id,
        computer_id,
        computer_id,
        identity,
        known_hosts,
        helper,
    )
}

/// The entry named after `entry_id` that connects to the computer `target_id`, with that
/// computer's key and pinned host key. They are the same computer except after a promote.
fn config_text_for(
    device_id: &str,
    entry_id: &str,
    target_id: &str,
    identity: &Path,
    known_hosts: &Path,
    helper: &Path,
) -> Result<String, String> {
    validate_id(device_id)?;
    validate_id(entry_id)?;
    validate_id(target_id)?;
    let identity = editor::ssh_quote(identity)?;
    let known_hosts = editor::ssh_quote(known_hosts)?;
    let helper = helper.to_str().ok_or(FAILED)?;
    if helper.chars().any(char::is_control) {
        return Err(FAILED.into());
    }
    let pinned = alias(target_id);
    let alias = alias(entry_id);
    let proxy = proxy_text(helper, device_id, target_id, None);
    Ok(format!(
        "Host {alias}\n  HostName {alias}\n  Port 22\n  User silo\n  IdentityFile {identity}\n  IdentitiesOnly yes\n  IdentityAgent none\n  UserKnownHostsFile {known_hosts}\n  GlobalKnownHostsFile /dev/null\n  HostKeyAlias {pinned}\n  StrictHostKeyChecking yes\n  UpdateHostKeys no\n  ForwardAgent no\n  ForwardX11 no\n  ClearAllForwardings yes\n  PermitLocalCommand no\n  ConnectTimeout 10\n  ServerAliveInterval 15\n  LogLevel ERROR\n  ProxyCommand {proxy}\n"
    ))
}

/// The other names a saved entry answers to (`Host silo-<id> main.silo`), which a client
/// added after the engine wrote it. Rewriting the entry keeps them.
fn extra_hosts(existing: &str) -> Vec<String> {
    let valid = |name: &str| {
        !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    };
    existing
        .lines()
        .find_map(|line| line.strip_prefix("Host "))
        .map(|names| {
            names
                .split_whitespace()
                .skip(1)
                .filter(|name| valid(name))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn with_extra_hosts(text: String, extras: &[String]) -> String {
    if extras.is_empty() {
        return text;
    }
    match text.split_once('\n') {
        Some((first, rest)) => format!("{first} {}\n{rest}", extras.join(" ")),
        None => text,
    }
}

fn saved_extras(config: &Path) -> Vec<String> {
    fs::read_to_string(config)
        .map(|text| extra_hosts(&text))
        .unwrap_or_default()
}

fn check_guest_path(
    layout: &super::store::Layout,
    record: &super::store::Record,
    path: &str,
) -> Result<(), String> {
    editor::validate_macos_path(path)?;
    let command = format!("test -d {}", shell_quote(path));
    let output = guest_access::run_capped(
        layout,
        record,
        &command,
        None,
        Duration::from_secs(8),
        Some(1024),
    )?;
    if output.status == 0 {
        Ok(())
    } else {
        Err("This folder is unavailable inside the computer.".into())
    }
}

fn probe(config: &Path, alias: &str) -> Result<(), String> {
    let mut command = Command::new(SSH);
    launch::sanitize_child(&mut command)
        .args([
            "-F",
            config.to_str().ok_or(FAILED)?,
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=5",
            alias,
            "true",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut task = crate::owned_tunnel::Tunnel::spawn_task(&command, None).map_err(|_| {
        "Could not connect to this computer over SSH. Retry after checking that it is running."
            .to_string()
    })?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match task.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) | Err(_) => return Err("Could not connect to this computer over SSH. Retry after checking that it is running.".into()),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            Ok(None) => {
                task.kill_group();
                return Err("Could not connect to this computer over SSH. Retry after checking that it is running.".into());
            }
        }
    }
}

pub(super) fn ensure(
    app: &MacosContext,
    computer_id: &str,
    requested_path: Option<&str>,
    install_include: Option<bool>,
) -> Result<GuestSshAliasEnsureResult, String> {
    validate_id(computer_id)?;
    editor::require_openssh("connect to this computer over SSH")?;
    super::ensure_loaded(app)?;
    let (existing, state) = super::computer(computer_id)?;
    if state != super::store::State::Running {
        return Err("Start this computer first.".into());
    }
    if !existing.installed || !existing.setup.complete() {
        return Err("Finish setting up this computer before connecting over SSH.".into());
    }
    let path = requested_path.unwrap_or(files::WORKSPACE);
    let (layout, record) = super::layout_and_record(app, computer_id)?;
    check_guest_path(&layout, &record, path)?;
    let account = guest_access::account(&layout)?;
    let device_id = crate::remote::log_identity()?.0;
    let config = config_path(app, computer_id);
    let root = config.parent().ok_or(FAILED)?;
    let include = editor::include_line(root)?;
    let text = with_extra_hosts(
        config_text(
            &device_id,
            computer_id,
            &account.key,
            &known_hosts(&layout),
            &proxy_helper()?,
        )?,
        &saved_extras(&config),
    );
    let _guard = lock();
    editor::private_directory(root)?;
    editor::write_private(&config, text.as_bytes())?;
    let name = alias(computer_id);
    probe(&config, &name)?;
    let include_installed = if install_include.unwrap_or(true) {
        editor::install_include(&app.paths().home, &include)?;
        true
    } else {
        editor::user_config_has_line(&app.paths().home, &include)
    };
    Ok(GuestSshAliasEnsureResult {
        alias: name,
        user: account.user,
        home: files::HOME.into(),
        config_path: config.to_string_lossy().into_owned(),
        include_line: include,
        include_installed,
    })
}

/// After a promote: the saved entry of `a` connects to `b` and the entry of `b` to `a`
/// (`swapped`), or each to its own computer again. Only entries that exist are rewritten, so
/// a client that saved main's alias reaches the new main. Repeating it changes nothing.
pub(super) fn point(app: &MacosContext, a: &str, b: &str, swapped: bool) -> Result<(), String> {
    validate_id(a)?;
    validate_id(b)?;
    let entries = [
        (a, if swapped { b } else { a }),
        (b, if swapped { a } else { b }),
    ];
    if !entries
        .iter()
        .any(|(entry, _)| config_path(app, entry).exists())
    {
        return Ok(());
    }
    let device_id = crate::remote::log_identity()?.0;
    let helper = proxy_helper()?;
    let data = app.paths().app_data.clone();
    for (entry, target) in entries {
        let config = config_path(app, entry);
        if !config.exists() {
            continue;
        }
        let layout = super::store::Layout::new(&data, target);
        let account = guest_access::account(&layout)?;
        let text = with_extra_hosts(
            config_text_for(
                &device_id,
                entry,
                target,
                &account.key,
                &known_hosts(&layout),
                &helper,
            )?,
            &saved_extras(&config),
        );
        let _guard = lock();
        editor::write_private(&config, text.as_bytes())?;
    }
    Ok(())
}

pub(super) fn remove(app: &MacosContext, computer_id: &str) -> Result<(), String> {
    validate_id(computer_id)?;
    let path = config_path(app, computer_id);
    let _guard = lock();
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            fs::remove_file(&path).map_err(|_| FAILED)?;
            if let Some(parent) = path.parent() {
                fs::File::open(parent)
                    .and_then(|directory| directory.sync_all())
                    .map_err(|_| FAILED)?;
            }
            Ok(())
        }
        Ok(_) => Err(FAILED.into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(FAILED.into()),
    }
}

pub(super) fn terminal_command(
    app: &MacosContext,
    computer_id: &str,
    agent: Option<AgentId>,
) -> Result<CommandResult, String> {
    let connection = ensure(app, computer_id, None, Some(true))?;
    Ok(CommandResult {
        argv: terminal_argv(Path::new(&connection.config_path), &connection.alias, agent)?,
    })
}

fn terminal_argv(
    config: &Path,
    alias: &str,
    agent: Option<AgentId>,
) -> Result<Vec<String>, String> {
    let mut argv = vec![
        SSH.into(),
        "-F".into(),
        config.to_str().ok_or(FAILED)?.into(),
        "-t".into(),
        alias.into(),
    ];
    if let Some(agent) = agent {
        argv.push(crate::agents::launch_script(agent, true));
    }
    Ok(argv)
}

pub(super) fn editor_command(
    app: &MacosContext,
    computer_id: &str,
    path: Option<&str>,
    command: &launch::EditorCommand,
) -> Result<CommandResult, String> {
    let path = path.unwrap_or(files::WORKSPACE);
    let connection = ensure(app, computer_id, Some(path), Some(true))?;
    let built = editor::macos_editor_launch(command, &connection.alias, path, &app.paths().home)?;
    let mut argv = vec![built.get_program().to_str().ok_or(FAILED)?.to_owned()];
    for argument in built.get_args() {
        argv.push(argument.to_str().ok_or(FAILED)?.to_owned());
    }
    Ok(CommandResult { argv })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alias_config_is_stable_by_id_and_pins_the_guest_without_forwarding() {
        let id = "e38c0e77-28bc-44a0-9b3d-3b4f68f0d85f";
        let config = config_text(
            "c690f54f-4cc9-4950-8782-30f21e89d18b",
            id,
            Path::new("/tmp/Silo Dev/guest-access/id_ed25519"),
            Path::new("/tmp/Silo Dev/guest-access/known_hosts"),
            Path::new("/Applications/Silo Dev.app/Contents/MacOS/silod"),
        )
        .unwrap();
        assert!(config.starts_with(&format!("Host silo-{id}\n")));
        assert!(config.contains("IdentityAgent none\n"));
        assert!(config.contains("StrictHostKeyChecking yes\n"));
        assert!(config.contains(&format!("HostKeyAlias silo-{id}\n")));
        assert!(config.contains("ForwardAgent no\n  ForwardX11 no\n  ClearAllForwardings yes\n"));
        assert!(config.contains(&format!(
            "--remote-guest 'c690f54f-4cc9-4950-8782-30f21e89d18b' '{id}'\n"
        )));
    }

    #[test]
    fn after_a_promote_main_s_entry_connects_to_the_new_main() {
        let (main, fork) = (
            "e38c0e77-28bc-44a0-9b3d-3b4f68f0d85f",
            "1f4f1f9a-7c6e-4a58-8a2e-0d8f3c2b9a11",
        );
        let config = config_text_for(
            "c690f54f-4cc9-4950-8782-30f21e89d18b",
            main,
            fork,
            Path::new("/tmp/fork/guest-access/id_ed25519"),
            Path::new("/tmp/fork/guest-access/known_hosts"),
            Path::new("/Applications/Silo Dev.app/Contents/MacOS/silod"),
        )
        .unwrap();
        // Saved connections keep the name they were saved with...
        assert!(config.starts_with(&format!("Host silo-{main}\n")));
        // ...and reach the new main, pinned to its host key.
        assert!(config.contains(&format!(
            "--remote-guest 'c690f54f-4cc9-4950-8782-30f21e89d18b' '{fork}'\n"
        )));
        assert!(config.contains(&format!("HostKeyAlias silo-{fork}\n")));
        assert!(config.contains("/tmp/fork/guest-access/known_hosts"));
    }

    #[test]
    fn names_added_to_an_entry_survive_rewriting_it() {
        let id = "e38c0e77-28bc-44a0-9b3d-3b4f68f0d85f";
        let written = config_text(
            "c690f54f-4cc9-4950-8782-30f21e89d18b",
            id,
            Path::new("/tmp/key"),
            Path::new("/tmp/known"),
            Path::new("/tmp/silod"),
        )
        .unwrap();
        assert!(extra_hosts(&written).is_empty());
        let edited = with_extra_hosts(written.clone(), &["main.silo-dev".to_owned()]);
        assert!(edited.starts_with(&format!("Host silo-{id} main.silo-dev\n  HostName")));
        let extras = extra_hosts(&edited);
        assert_eq!(extras, ["main.silo-dev"]);
        assert_eq!(with_extra_hosts(written, &extras), edited);
        assert!(!extra_hosts("Host silo-x bad;name ok.silo\n").contains(&"bad;name".to_owned()));
    }

    #[test]
    fn alias_config_rejects_non_id_keys() {
        assert!(config_text(
            "../escape",
            "d1c418da-7f4d-49f5-a490-340365b0d8fb",
            Path::new("/tmp/key"),
            Path::new("/tmp/known"),
            Path::new("/tmp/silod")
        )
        .is_err());
    }

    #[test]
    fn terminal_command_is_an_argv_for_the_client_to_launch() {
        let argv = terminal_argv(
            Path::new("/tmp/Silo Dev/ssh/macos.conf"),
            &alias("e38c0e77-28bc-44a0-9b3d-3b4f68f0d85f"),
            None,
        )
        .unwrap();
        assert_eq!(
            argv,
            [
                SSH,
                "-F",
                "/tmp/Silo Dev/ssh/macos.conf",
                "-t",
                "silo-e38c0e77-28bc-44a0-9b3d-3b4f68f0d85f",
            ]
        );
    }

    #[test]
    fn a_terminal_with_an_agent_adds_one_argument_with_the_fixed_command() {
        for agent in [AgentId::ClaudeCode, AgentId::Codex] {
            let argv = terminal_argv(
                Path::new("/tmp/Silo Dev/ssh/macos.conf"),
                &alias("e38c0e77-28bc-44a0-9b3d-3b4f68f0d85f"),
                Some(agent),
            )
            .unwrap();
            assert_eq!(argv.len(), 6);
            assert_eq!(argv[5], crate::agents::launch_script(agent, true));
            assert!(argv[5].starts_with("cd /Users/silo/workspace "));
        }
    }
}
