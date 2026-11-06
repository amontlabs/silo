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
    config::{Fold, Settings},
    engine::{Engine, Progress},
    error::CliError,
};

/// The policy the settings give an agent: its `fold` mode and the secrets a fork may name.
pub fn policy_from(settings: Option<&Settings>, agent: Option<&str>) -> GuestClientPolicy {
    let settings = settings.cloned().unwrap_or_default();
    GuestClientPolicy {
        fold: match settings.fold {
            Fold::Ask => "ask",
            Fold::AutoCode => "auto-code",
        }
        .into(),
        fork_secrets: settings.fork_secrets,
        agent: agent.map(str::to_owned),
    }
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
    if let Some(binary) = binary {
        upload_binary(engine, &target.id, &target.name, binary, progress).await?;
        installed.push("silo binary at /usr/local/bin/silo");
    }
    Ok(json!({
        "computer": target.name,
        "installed": installed,
        "tokenPath": GUEST_TOKEN_PATH,
        "mintedAt": enabled["mintedAt"],
    }))
}

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
        "/workspace/silo-in-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..12]
    );
    engine
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
        .await?;
    as_root(
        engine,
        computer_id,
        "set -e; install -m 0755 \"$1/$2\" /usr/local/bin/silo; rm -rf \"$1\"",
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
