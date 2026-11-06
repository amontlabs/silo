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

/// Run as root after the upload. `$1` is the uploaded file in the account's `/workspace`, `$2`
/// the sha256 the host computed of the local binary. Nothing under `/workspace` is changed as
/// root: the source is refused if it is a link or not a regular file, copied into a folder only
/// root can enter, and installed only if the copy's hash is the host's.
const INSTALL_SCRIPT: &str = "set -e; src=\"$1\"; want=\"$2\"; \
     if [ -L \"$src\" ] || [ ! -f \"$src\" ]; then echo 'The uploaded file is not a regular file.' >&2; exit 1; fi; \
     root_tmp=$(mktemp -d /root/silo-install.XXXXXX); chmod 700 \"$root_tmp\"; \
     trap 'rm -rf -- \"$root_tmp\"' EXIT; \
     cat -- \"$src\" > \"$root_tmp/silo\"; \
     got=$(sha256sum -- \"$root_tmp/silo\" | cut -d' ' -f1); \
     if [ \"$got\" != \"$want\" ]; then echo 'The uploaded file does not match the local binary (sha256).' >&2; exit 1; fi; \
     install -m 0755 -o root -g root \"$root_tmp/silo\" /usr/local/bin/silo";

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

async fn upload_binary(
    engine: &Engine,
    computer_id: &str,
    name: &str,
    binary: &Path,
    progress: Progress<'_>,
) -> Result<(), CliError> {
    let bytes = std::fs::read(binary)
        .map_err(|error| CliError::new(format!("--binary cannot be read: {error}")))?;
    let hash = sha256_hex(&bytes);
    // A fresh name, so the upload never lands on a file that already exists in /workspace.
    let file_name = format!(
        "silo-install-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..12]
    );
    let staging =
        std::env::temp_dir().join(format!("silo-stage-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&staging)
        .map_err(|error| CliError::new(format!("The binary could not be staged: {error}")))?;
    let staged = staging.join(&file_name);
    let written = std::fs::write(&staged, &bytes);
    drop(bytes);
    if let Err(error) = written {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(CliError::new(format!(
            "The binary could not be staged: {error}"
        )));
    }
    let uploaded = engine
        .run_job(
            &format!("Sending the silo binary to {name}"),
            requests::files_upload(FileUploadParams {
                transfer_id: crate::folder::transfer_id(),
                computer_id: computer_id.to_owned(),
                device_id: None,
                directory: "/workspace".to_owned(),
                conflict: ConflictPolicy::Replace,
                selection: None,
                paths: Some(vec![staged.to_string_lossy().into_owned()]),
                origin: None,
                connection_id: None,
            }),
            progress,
        )
        .await;
    let _ = std::fs::remove_dir_all(&staging);
    let remote = format!("/workspace/{file_name}");
    let installed = match uploaded {
        Ok(_) => as_root(engine, computer_id, INSTALL_SCRIPT, &[&remote, &hash], None).await,
        Err(error) => Err(error),
    };
    // The uploaded file belongs to the account, so the account removes it.
    let _ = engine
        .call(requests::guest_exec(GuestExecParams {
            computer_id: computer_id.to_owned(),
            device_id: None,
            argv: vec!["rm".into(), "-f".into(), "--".into(), remote],
            cwd: None,
            env: None,
            as_user: None,
            stdin: None,
            timeout_ms: 60_000,
            max_output_bytes: None,
        }))
        .await;
    installed
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
mod install_script_tests {
    use super::{sha256_hex, INSTALL_SCRIPT};

    #[test]
    fn the_script_refuses_links_checks_the_hash_and_leaves_workspace_alone() {
        assert!(INSTALL_SCRIPT.contains("[ -L \"$src\" ]"));
        assert!(INSTALL_SCRIPT.contains("[ ! -f \"$src\" ]"));
        assert!(INSTALL_SCRIPT.contains("sha256sum"));
        assert!(INSTALL_SCRIPT.contains("\"$got\" != \"$want\""));
        assert!(INSTALL_SCRIPT.contains("mktemp -d /root/"));
        assert!(INSTALL_SCRIPT.contains("install -m 0755 -o root -g root \"$root_tmp/silo\""));
        for word in ["chown", "chmod /workspace", "chgrp"] {
            assert!(!INSTALL_SCRIPT.contains(word), "{word}");
        }
    }

    #[test]
    fn the_hash_is_lowercase_hex() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn the_script_runs_as_a_plain_shell_script_and_rejects_a_link_and_a_wrong_hash() {
        use std::process::Command;
        let base = std::env::temp_dir().join(format!("silo-inst-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let real = base.join("real");
        std::fs::write(&real, b"abc").unwrap();
        let link = base.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let run = |src: &std::path::Path, hash: &str| {
            Command::new("sh")
                .args(["-c", INSTALL_SCRIPT, "silo-install"])
                .arg(src)
                .arg(hash)
                .output()
                .unwrap()
        };
        let link_out = run(&link, &sha256_hex(b"abc"));
        assert!(!link_out.status.success());
        assert!(String::from_utf8_lossy(&link_out.stderr).contains("not a regular file"));
        let wrong = run(&real, &"0".repeat(64));
        assert!(!wrong.status.success());
        std::fs::remove_dir_all(&base).unwrap();
    }
}
