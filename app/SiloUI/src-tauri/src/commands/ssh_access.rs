//! SSH access settings of the computers, through `sshAccess.*`. The save dialog for an
//! exported key stays on this device: it picks the path, the engine writes the key there.

use serde_json::Value;
use silo_api::methods::{
    requests,
    ssh_access::{SshAccessConnectionParams, SshAccessSaveParams},
};
use tauri::{AppHandle, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

use crate::engine_client::{params::device, Engine, EngineClient, EngineError};

pub(crate) async fn state(engine: &EngineClient) -> Result<Value, EngineError> {
    engine.call(requests::ssh_access_state(device(None))).await
}

pub(crate) async fn save(
    engine: &EngineClient,
    computer: &str,
    enabled: bool,
    port: u16,
    bind_address: String,
    keys: Option<Vec<String>>,
) -> Result<Value, EngineError> {
    engine
        .call(requests::ssh_access_save(SshAccessSaveParams {
            computer_id: engine.computer_id(computer).await?,
            device_id: None,
            enabled,
            port,
            bind_address,
            keys,
        }))
        .await
}

/// Which computer a connection export is for: a name on this device, or a computer of a
/// connected device.
pub(crate) struct ConnectionTarget {
    pub computer: Option<String>,
    pub device_id: Option<String>,
    pub computer_id: Option<String>,
}

impl ConnectionTarget {
    /// The name the exported key file suggests.
    fn key_filename(&self) -> String {
        let name = self
            .computer
            .as_deref()
            .or(self.computer_id.as_deref())
            .unwrap_or("connection");
        let safe: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        format!("silo-{safe}.key")
    }
}

/// The SSH command (`download` false) or the key written to `destination` (`download` true).
pub(crate) async fn connection(
    engine: &EngineClient,
    target: &ConnectionTarget,
    download: bool,
    network: Option<bool>,
    destination: Option<String>,
) -> Result<Option<String>, EngineError> {
    let computer_id = match (&target.device_id, &target.computer_id, &target.computer) {
        (Some(_), Some(id), _) => id.clone(),
        (Some(_), None, _) => return Err(EngineError::internal("Missing computer identity.")),
        (None, _, Some(name)) => engine.computer_id(name).await?,
        (None, _, None) => return Err(EngineError::internal("Missing computer name.")),
    };
    let destination = if download {
        Some(destination.ok_or_else(|| EngineError::internal("Choose where to save the key."))?)
    } else {
        None
    };
    let reply = engine
        .call(requests::ssh_access_connection(SshAccessConnectionParams {
            computer_id,
            device_id: target.device_id.clone(),
            network,
            download,
            destination,
        }))
        .await?;
    Ok(reply.as_str().filter(|_| !download).map(str::to_owned))
}

#[tauri::command]
pub(crate) async fn read_ssh_access_state(engine: Engine<'_>) -> Result<Value, EngineError> {
    state(&engine).await
}

#[tauri::command]
pub(crate) async fn save_ssh_access(
    engine: Engine<'_>,
    computer: String,
    enabled: bool,
    port: u16,
    bind_address: String,
    keys: Option<Vec<String>>,
) -> Result<Value, EngineError> {
    save(&engine, &computer, enabled, port, bind_address, keys).await
}

#[tauri::command]
pub(crate) async fn ssh_connection(
    engine: Engine<'_>,
    app: AppHandle,
    window: WebviewWindow,
    computer: Option<String>,
    device_id: Option<String>,
    computer_id: Option<String>,
    download: bool,
    network: Option<bool>,
) -> Result<Option<String>, EngineError> {
    if window.label() != "main" {
        return Err(EngineError::internal(
            "SSH keys can only be exported from the main window.",
        ));
    }
    let target = ConnectionTarget {
        computer,
        device_id,
        computer_id,
    };
    let destination = if download {
        let filename = target.key_filename();
        let dialog_window = window.clone();
        let chosen = tokio::task::spawn_blocking(move || {
            app.dialog()
                .file()
                .set_parent(&dialog_window)
                .set_title("Save SSH connection key")
                .set_file_name(filename)
                .blocking_save_file()
        })
        .await
        .map_err(|_| EngineError::internal("Could not prepare SSH connection."))?;
        let Some(chosen) = chosen else {
            return Ok(None);
        };
        let path = chosen
            .into_path()
            .map_err(|_| EngineError::internal("Choose a local file."))?;
        Some(
            path.to_str()
                .ok_or_else(|| EngineError::internal("Choose a local file."))?
                .to_owned(),
        )
    } else {
        None
    };
    connection(&engine, &target, download, network, destination).await
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};
    use serde_json::json;
    use silo_api::methods::MethodName;

    const DEV: &str = "00000000-0000-4000-8000-00000000000d";
    const DEVICE: &str = "00000000-0000-4000-8000-0000000000d1";

    fn replies(method: MethodName, params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            MethodName::EventsSubscribe => Ok(json!({"subscription": "sub"})),
            MethodName::ComputersList => Ok(json!({
                "linux": {"computers": [{"configuration": {"id": DEV, "name": "dev"}}]},
                "macos": {"computers": []}
            })),
            MethodName::SshAccessState | MethodName::SshAccessSave => Ok(json!({"computers": []})),
            MethodName::SshAccessConnection if params["download"] == true => Ok(Value::Null),
            MethodName::SshAccessConnection => Ok(json!("ssh -p 2222 user@127.0.0.1")),
            _ => Ok(Value::Null),
        }
    }

    #[tokio::test]
    async fn access_is_read_and_saved_by_computer_id() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        state(&engine).await.unwrap();
        save(
            &engine,
            "dev",
            true,
            2222,
            "127.0.0.1".into(),
            Some(vec!["k".into()]),
        )
        .await
        .unwrap();
        save(&engine, "dev", false, 2222, "127.0.0.1".into(), None)
            .await
            .unwrap();
        assert_eq!(
            fake.calls(MethodName::SshAccessSave),
            vec![
                json!({"computerId": DEV, "enabled": true, "port": 2222, "bindAddress": "127.0.0.1", "keys": ["k"]}),
                json!({"computerId": DEV, "enabled": false, "port": 2222, "bindAddress": "127.0.0.1"}),
            ]
        );
    }

    #[tokio::test]
    async fn a_connection_returns_the_command_and_a_download_returns_nothing() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let local = ConnectionTarget {
            computer: Some("dev".into()),
            device_id: None,
            computer_id: None,
        };
        assert_eq!(
            connection(&engine, &local, false, Some(true), None)
                .await
                .unwrap(),
            Some("ssh -p 2222 user@127.0.0.1".to_owned())
        );
        let remote = ConnectionTarget {
            computer: None,
            device_id: Some(DEVICE.into()),
            computer_id: Some(DEV.into()),
        };
        assert_eq!(
            connection(&engine, &remote, true, None, Some("/tmp/k.key".into()))
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            fake.calls(MethodName::SshAccessConnection),
            vec![
                json!({"computerId": DEV, "download": false, "network": true}),
                json!({"computerId": DEV, "deviceId": DEVICE, "download": true, "destination": "/tmp/k.key"}),
            ]
        );
        // A download needs the path the dialog chose.
        assert!(connection(&engine, &remote, true, None, None)
            .await
            .is_err());
        assert_eq!(remote.key_filename(), format!("silo-{DEV}.key"));
    }

    #[tokio::test]
    async fn unsupported_remote_access_is_the_typed_unavailable_state() {
        let fake = FakeEngine::start(|method, params| match method {
            MethodName::SshAccessConnection => Err((
                "unsupported_remote_operation",
                "Not available for that device yet.".into(),
            )),
            _ => replies(method, params),
        });
        let (engine, _) = connected_client(&fake).await;
        let remote = ConnectionTarget {
            computer: None,
            device_id: Some(DEVICE.into()),
            computer_id: Some(DEV.into()),
        };
        let error = connection(&engine, &remote, false, None, None)
            .await
            .unwrap_err();
        assert_eq!(error.code, "unsupported_remote_operation");
    }
}
