//! `silo guest-client install|revoke|status <computer>`: the host side of the in-guest client.
//! The settings are read here, from the project the command runs in, and travel to the engine
//! as the token's policy, because the engine has no project folder of its own.

use std::path::Path;

use serde_json::{json, Value};
use silo_api::methods::{
    files::{ConflictPolicy, FileUploadParams},
    guest::{GuestExecParams, GuestExecUser},
    guest_client::{
        GuestClientComputerParams, GuestClientEnableParams, GuestClientPolicy, GUEST_TOKEN_PATH,
    },
    requests,
};

use crate::{
    config::Settings,
    engine::{Engine, Progress},
    error::CliError,
};

/// The policy the settings give an agent: the secrets a fork made from inside may name. The
/// in-guest client never keeps, so `fold` and `auto_keep` do not enter it.
pub fn policy_from(settings: Option<&Settings>, agent: Option<&str>) -> GuestClientPolicy {
    let settings = settings.cloned().unwrap_or_default();
    GuestClientPolicy {
        fork_secrets: settings.fork_secrets,
        agent: agent.map(str::to_owned),
    }
}

/// A warning when the settings now differ from what the token was minted with.
pub fn stale_warning(status: &Value, current: &GuestClientPolicy) -> Option<String> {
    let stored: Vec<String> = status["policy"]["forkSecrets"]
        .as_array()?
        .iter()
        .filter_map(|name| name.as_str().map(str::to_owned))
        .collect();
    let (mut stored, mut now) = (stored, current.fork_secrets.clone());
    stored.sort();
    now.sort();
    (stored != now).then(|| {
        format!(
            "The token was minted with fork_secrets {stored:?} but the settings now say {now:?}. Run `silo guest-client install` again to apply them."
        )
    })
}

fn valid_group(group: &str) -> bool {
    !group.is_empty()
        && group.len() <= 32
        && group.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
        })
}

async fn as_root(
    engine: &Engine,
    computer_id: &str,
    script: &str,
    arguments: &[&str],
    stdin: Option<Vec<u8>>,
) -> Result<(), CliError> {
    let mut argv: Vec<String> = vec![
        "sh".into(),
        "-c".into(),
        script.into(),
        "silo-install".into(),
    ];
    argv.extend(arguments.iter().map(|argument| (*argument).to_owned()));
    let reply = engine
        .call(requests::guest_exec(GuestExecParams {
            computer_id: computer_id.to_owned(),
            device_id: None,
            argv,
            cwd: None,
            env: None,
            as_user: Some(GuestExecUser::Root),
            stdin,
            timeout_ms: 60_000,
            max_output_bytes: None,
        }))
        .await?;
    if reply["exitCode"].as_i64() == Some(0) {
        return Ok(());
    }
    let stderr: Vec<u8> = reply["stderr"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_u64().map(|byte| byte as u8))
                .collect()
        })
        .unwrap_or_default();
    Err(CliError::new(format!(
        "The install step failed in the computer: {}",
        String::from_utf8_lossy(&stderr).trim()
    )))
}

pub async fn install(
    engine: &Engine,
    computer: &str,
    policy: GuestClientPolicy,
    binary: Option<&Path>,
    group: &str,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    if !valid_group(group) {
        return Err(CliError::new("--group must be a Unix group name."));
    }
    let target = engine.computer(computer).await?;
    if target.kind != "linux" {
        return Err(CliError::new(
            "The in-guest client is available for Linux computers only.",
        ));
    }
    if target.state != "running" {
        return Err(CliError::new(format!(
            "Start {computer} first; the token is written into it."
        )));
    }
    // The binary goes first, so an upload that fails leaves no token behind.
    if let Some(binary) = binary {
        upload_binary(engine, &target.id, &target.name, binary, progress).await?;
    }
    let enabled = engine
        .call(requests::guest_client_enable(GuestClientEnableParams {
            computer_id: target.id.clone(),
            policy,
        }))
        .await?;
    let file = json!({
        "version": 1,
        "token": enabled["token"],
        "computerId": target.id,
        "spool": enabled["spoolDir"],
    });
    let script = "set -e; umask 077; mkdir -p /etc/silo; chmod 755 /etc/silo; \
        cat > \"$1.new\"; chmod 640 \"$1.new\"; \
        if getent group \"$2\" >/dev/null 2>&1; then chgrp \"$2\" \"$1.new\"; else chmod 600 \"$1.new\"; fi; \
        mv \"$1.new\" \"$1\"";
    let written = as_root(
        engine,
        &target.id,
        script,
        &[GUEST_TOKEN_PATH, group],
        Some(file.to_string().into_bytes()),
    )
    .await;
    if let Err(error) = written {
        // A token nobody could install is not left valid.
        let _ = engine
            .call(requests::guest_client_revoke(GuestClientComputerParams {
                computer_id: target.id.clone(),
            }))
            .await;
        return Err(error);
    }
    let mut installed = vec!["token"];
    if binary.is_some() {
        installed.push("silo binary at /usr/local/bin/silo");
    }
    Ok(json!({
        "computer": target.name,
        "installed": installed,
        "tokenPath": GUEST_TOKEN_PATH,
        "mintedAt": enabled["mintedAt"],
    }))
}

/// Makes the upload folder as root, directly in the sticky `/tmp` where the working account
/// cannot rename or replace entries it does not own: `mkdir` fails on a name that exists (a link
/// included), and the folder is handed to the owner of `$2` and checked to be a real folder.
const UPLOAD_FOLDER_SCRIPT: &str = "set -e; mkdir -m 700 -- \"$1\"; \
     chown -h \"$(stat -L -c %u:%g -- \"$2\")\" \"$1\"; [ -d \"$1\" ] && [ ! -L \"$1\" ]";

async fn upload_binary(
    engine: &Engine,
    computer_id: &str,
    name: &str,
    binary: &Path,
    progress: Progress<'_>,
) -> Result<(), CliError> {
    let file_name = binary
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.contains('/') && !name.is_empty())
        .ok_or_else(|| CliError::new("--binary must name a file."))?;
    let directory = format!(
        "/tmp/silo-in-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..12]
    );
    // The transfer needs the folder to exist and the account that receives it to own it.
    as_root(
        engine,
        computer_id,
        UPLOAD_FOLDER_SCRIPT,
        &[&directory, "/workspace"],
        None,
    )
    .await?;
    let uploaded = engine
        .run_job(
            &format!("Sending the silo binary to {name}"),
            requests::files_upload(FileUploadParams {
                transfer_id: crate::folder::transfer_id(),
                computer_id: computer_id.to_owned(),
                device_id: None,
                directory: directory.clone(),
                conflict: ConflictPolicy::Replace,
                selection: None,
                paths: Some(vec![binary.to_string_lossy().into_owned()]),
                origin: None,
                connection_id: None,
            }),
            progress,
        )
        .await;
    if let Err(error) = uploaded {
        // The folder is removed whether or not the transfer got anywhere.
        let _ = as_root(engine, computer_id, "rm -rf \"$1\"", &[&directory], None).await;
        return Err(error);
    }
    as_root(
        engine,
        computer_id,
        // The upload folder belongs to the working account, so the file is copied into a folder
        // only root can enter, checked there, and installed from there.
        "set -e; src=\"$1/$2\"; [ -f \"$src\" ] && [ ! -L \"$src\" ]; \
         safe=$(mktemp -d /var/lib/silo-install.XXXXXX); chmod 700 \"$safe\"; \
         cp -P \"$src\" \"$safe/silo\"; rm -rf \"$1\"; [ -f \"$safe/silo\" ] && [ ! -L \"$safe/silo\" ]; \
         install -m 0755 -o root -g root \"$safe/silo\" /usr/local/bin/silo; rm -rf \"$safe\"",
        &[&directory, file_name],
        None,
    )
    .await
}

pub async fn revoke(engine: &Engine, computer: &str) -> Result<Value, CliError> {
    let target = engine.computer(computer).await?;
    engine
        .call(requests::guest_client_revoke(GuestClientComputerParams {
            computer_id: target.id,
        }))
        .await
}

pub async fn status(engine: &Engine, computer: &str) -> Result<Value, CliError> {
    let target = engine.computer(computer).await?;
    engine
        .call(requests::guest_client_status(GuestClientComputerParams {
            computer_id: target.id,
        }))
        .await
}

#[cfg(test)]
mod upload_folder_tests {
    use super::UPLOAD_FOLDER_SCRIPT;
    use std::process::Command;

    fn run(folder: &std::path::Path, owner: &std::path::Path) -> bool {
        Command::new("sh")
            .args(["-c", UPLOAD_FOLDER_SCRIPT, "silo-install"])
            .arg(folder)
            .arg(owner)
            .status()
            .unwrap()
            .success()
    }

    #[test]
    fn the_upload_folder_is_new_private_and_never_a_link() {
        let base = std::env::temp_dir().join(format!("silo-up-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let folder = base.join("silo-in-1");
        assert!(run(&folder, &base));
        assert!(std::fs::metadata(&folder).unwrap().is_dir());
        // A name that already exists, as a folder or as a link, is refused.
        assert!(!run(&folder, &base));
        let link = base.join("silo-in-2");
        std::os::unix::fs::symlink(&base, &link).unwrap();
        assert!(!run(&link, &base));
        std::fs::remove_dir_all(&base).unwrap();
    }
}
