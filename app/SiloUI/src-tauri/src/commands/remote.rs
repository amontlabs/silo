//! Connections and the computers of connected devices, through `connections.*` and the
//! domain methods with a `deviceId`. The local engine routes a request to the device that
//! owns the computer; when it cannot, it answers `unsupported_remote_operation` and that
//! reaches the UI as the unavailable state. Opening terminals and browsers stays here.
use crate::engine_client::{
    forwarding::{browser, open_in_terminal, open_url, terminal, with_device},
    Engine, EngineClient, EngineError,
};
use serde_json::{json, Map, Value};
use tauri::AppHandle;

/// How a new computer starts: it asks before an agent acts unless the setting says auto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Approval {
    Ask,
    Auto,
}

impl Approval {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Auto => "auto",
        }
    }
}

fn approval_from_settings(settings: Result<Map<String, Value>, EngineError>) -> Approval {
    match settings
        .ok()
        .and_then(|settings| settings.get("computerUseAutoApproval").cloned())
    {
        Some(Value::Bool(true)) => Approval::Auto,
        _ => Approval::Ask,
    }
}

// MARK: Connections

pub(crate) async fn status(engine: &EngineClient) -> Result<Value, EngineError> {
    engine.call("connections.status", json!({})).await
}

pub(crate) async fn set_enabled(
    engine: &EngineClient,
    enabled: bool,
) -> Result<Value, EngineError> {
    engine
        .call("connections.setEnabled", json!({"enabled": enabled}))
        .await
}

pub(crate) async fn devices(engine: &EngineClient) -> Result<Value, EngineError> {
    engine.call("connections.devices.list", json!({})).await
}

pub(crate) async fn remove(engine: &EngineClient, device_id: String) -> Result<(), EngineError> {
    engine
        .call("connections.devices.remove", json!({"deviceId": device_id}))
        .await?;
    Ok(())
}

pub(crate) async fn connect(
    engine: &EngineClient,
    address: String,
    replace: Option<bool>,
) -> Result<Value, EngineError> {
    let mut params = json!({"address": address});
    if let Some(replace) = replace {
        params["replace"] = Value::Bool(replace);
    }
    engine.call("connections.devices.connect", params).await
}

/// Runs the command the engine built to authorize this device on `address` in a terminal.
pub(crate) async fn run_ssh_command(
    engine: &EngineClient,
    method: &str,
    address: String,
    open: impl FnOnce(&Map<String, Value>, &str) -> Result<(), String>,
) -> Result<(), EngineError> {
    let built = engine.call(method, json!({"address": address})).await?;
    let command = built
        .get("command")
        .and_then(Value::as_str)
        .ok_or_else(|| EngineError::internal("The engine returned no command."))?;
    open_in_terminal(engine, command, open).await
}

#[tauri::command]
pub(crate) async fn connections_status(engine: Engine<'_>) -> Result<Value, EngineError> {
    status(&engine).await
}

#[tauri::command]
pub(crate) async fn set_connections_enabled(
    engine: Engine<'_>,
    enabled: bool,
) -> Result<Value, EngineError> {
    set_enabled(&engine, enabled).await
}

#[tauri::command]
pub(crate) async fn device_list(engine: Engine<'_>) -> Result<Value, EngineError> {
    devices(&engine).await
}

#[tauri::command]
pub(crate) async fn remove_device(
    engine: Engine<'_>,
    device_id: String,
) -> Result<(), EngineError> {
    remove(&engine, device_id).await
}

#[tauri::command]
pub(crate) async fn authorize_device(
    engine: Engine<'_>,
    address: String,
) -> Result<(), EngineError> {
    run_ssh_command(
        &engine,
        "connections.ssh.authorizationCommand",
        address,
        terminal,
    )
    .await
}

#[tauri::command]
pub(crate) async fn setup_device_key(
    engine: Engine<'_>,
    address: String,
) -> Result<(), EngineError> {
    run_ssh_command(
        &engine,
        "connections.ssh.keySetupCommand",
        address,
        terminal,
    )
    .await
}

#[tauri::command]
pub(crate) async fn connect_device(
    engine: Engine<'_>,
    address: String,
    replace: Option<bool>,
) -> Result<Value, EngineError> {
    connect(&engine, address, replace).await
}

// MARK: Computers of a connected device

/// The Linux application state of a connected device.
pub(crate) async fn snapshot(
    engine: &EngineClient,
    device_id: &str,
    refresh_repositories: bool,
) -> Result<Value, EngineError> {
    let mut params = json!({});
    if refresh_repositories {
        params["refreshRepositories"] = Value::Bool(true);
    }
    let listed = engine
        .call("computers.list", with_device(params, Some(device_id)))
        .await?;
    listed
        .get("linux")
        .cloned()
        .ok_or_else(|| EngineError::internal("The device returned no computer state."))
}

/// Runs one lifecycle action on a computer of a connected device and returns that
/// device's state afterwards.
pub(crate) async fn computer_action(
    engine: &EngineClient,
    device_id: &str,
    computer_id: &str,
    action: &str,
) -> Result<Value, EngineError> {
    let params = json!({"computerId": computer_id, "deviceId": device_id});
    match action {
        "dismiss-error" => return engine.call("computers.dismissError", params).await,
        "start" | "stop" | "restart" => {
            engine
                .run_job(&format!("computers.{action}"), params)
                .await?;
        }
        other => {
            return Err(EngineError::internal(format!(
                "Silo does not know the computer action {other}."
            )))
        }
    }
    snapshot(engine, device_id, false).await
}

/// Runs one checkpoint action on a computer of a connected device.
pub(crate) async fn checkpoint_action(
    engine: &EngineClient,
    device_id: &str,
    computer_id: &str,
    action: &str,
    name: Option<String>,
    checkpoint_id: Option<String>,
    new_name: Option<String>,
) -> Result<(), EngineError> {
    let mut params = json!({"computerId": computer_id, "deviceId": device_id});
    let method = match action {
        "create" => {
            params["name"] = Value::String(
                name.ok_or_else(|| EngineError::internal("Missing checkpoint name."))?,
            );
            "checkpoints.create"
        }
        "fork" => {
            params["newName"] =
                Value::String(new_name.ok_or_else(|| EngineError::internal("Missing fork name."))?);
            if let Some(checkpoint_id) = checkpoint_id {
                params["checkpointId"] = Value::String(checkpoint_id);
            }
            "checkpoints.fork"
        }
        "restore" => {
            params["checkpointId"] = Value::String(
                checkpoint_id
                    .ok_or_else(|| EngineError::internal("Missing checkpoint identity."))?,
            );
            "checkpoints.restore"
        }
        _ => return Err(EngineError::internal("Unsupported checkpoint operation.")),
    };
    engine.run_job(method, params).await?;
    Ok(())
}

/// Saves a computer's configuration on a connected device. A new computer then gets the
/// approval mode this device prefers for new computers.
pub(crate) async fn upsert_computer(
    engine: &EngineClient,
    device_id: &str,
    configuration: Value,
    expected: Option<Value>,
) -> Result<Value, EngineError> {
    let created = expected.is_none();
    let computer_id = configuration
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| EngineError::internal("The computer has no identity."))?
        .to_owned();
    let mut change = json!({"kind": "upsert", "configuration": configuration});
    if let Some(expected) = expected {
        change["expected"] = expected;
    }
    engine
        .run_job(
            "computers.configuration.change",
            json!({"change": change, "deviceId": device_id}),
        )
        .await?;
    if created {
        // The preference is read after the computer was saved; failing to read it means ask.
        let settings = engine.call("settings.read", json!({})).await.map(|read| {
            read.get("settings")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default()
        });
        let mode = approval_from_settings(settings).as_str().to_owned();
        if let Err(error) = engine
            .call(
                "computerUse.approval",
                json!({"computerId": computer_id, "deviceId": device_id, "mode": mode}),
            )
            .await
        {
            eprintln!("Computer use approval was not set on the new remote computer: {error}");
        }
    }
    snapshot(engine, device_id, false).await
}

pub(crate) async fn delete_computer(
    engine: &EngineClient,
    device_id: &str,
    computer_id: &str,
    expected: Value,
) -> Result<Value, EngineError> {
    engine
        .run_job(
            "computers.delete",
            json!({"computerId": computer_id, "deviceId": device_id, "expected": expected}),
        )
        .await?;
    snapshot(engine, device_id, false).await
}

/// Display name of one computer in a device's application state.
fn computer_name(state: &Value, computer_id: &str) -> Option<String> {
    state["computers"]
        .as_array()?
        .iter()
        .find(|computer| computer["configuration"]["id"] == computer_id)
        .and_then(|computer| computer["configuration"]["name"].as_str())
        .map(str::to_owned)
}

fn notice_computer(
    device_id: &str,
    computer_id: &str,
    name: &str,
) -> crate::notifications::NoticeComputer {
    crate::notifications::NoticeComputer {
        id: format!("silo-remote:{device_id}:{computer_id}"),
        name: name.into(),
    }
}

#[tauri::command]
pub(crate) async fn device_snapshot(
    engine: Engine<'_>,
    device_id: String,
    refresh_repositories: Option<bool>,
) -> Result<Value, EngineError> {
    snapshot(&engine, &device_id, refresh_repositories.unwrap_or(false)).await
}

#[tauri::command]
pub(crate) async fn remote_computer_action(
    engine: Engine<'_>,
    app: AppHandle,
    device_id: String,
    computer_id: String,
    action: String,
    name: Option<String>,
) -> Result<Value, EngineError> {
    let started = std::time::Instant::now();
    let result = computer_action(&engine, &device_id, &computer_id, &action).await;
    let name = name
        .or_else(|| {
            result
                .as_ref()
                .ok()
                .and_then(|state| computer_name(state, &computer_id))
        })
        .unwrap_or_else(|| "this computer".into());
    let computer = notice_computer(&device_id, &computer_id, &name);
    let outcome = match &result {
        Ok(_) => crate::notifications::Outcome::Succeeded,
        Err(error) if error.is("cancelled") => crate::notifications::Outcome::Cancelled,
        Err(error) if error.is("already_queued") => crate::notifications::Outcome::AlreadyQueued,
        Err(error) => crate::notifications::Outcome::Failed(&error.message),
    };
    if let Some(notice) = crate::notifications::lifecycle_notice(
        &action,
        &name,
        Some(computer),
        started.elapsed(),
        outcome,
    ) {
        crate::commands::notifications::notify_native(&app, notice);
    }
    result
}

#[tauri::command]
pub(crate) async fn remote_checkpoint_action(
    engine: Engine<'_>,
    device_id: String,
    computer_id: String,
    action: String,
    name: Option<String>,
    checkpoint_id: Option<String>,
    new_name: Option<String>,
) -> Result<(), EngineError> {
    checkpoint_action(
        &engine,
        &device_id,
        &computer_id,
        &action,
        name,
        checkpoint_id,
        new_name,
    )
    .await
}

#[tauri::command]
pub(crate) async fn remote_upsert_computer(
    engine: Engine<'_>,
    device_id: String,
    configuration: Value,
    expected: Option<Value>,
) -> Result<Value, EngineError> {
    upsert_computer(&engine, &device_id, configuration, expected).await
}

#[tauri::command]
pub(crate) async fn remote_delete_computer(
    engine: Engine<'_>,
    app: AppHandle,
    device_id: String,
    computer_id: String,
    expected: Value,
) -> Result<Value, EngineError> {
    let deleted = format!("silo-remote:{device_id}:{computer_id}");
    let result = delete_computer(&engine, &device_id, &computer_id, expected).await;
    if result.is_ok() {
        crate::commands::notifications::clear_computer(&app, &deleted);
    }
    result
}

// MARK: Network and SSH access of a connected device

pub(crate) async fn network_state(
    engine: &EngineClient,
    device_id: &str,
) -> Result<Value, EngineError> {
    engine
        .call("network.state", with_device(json!({}), Some(device_id)))
        .await
}

pub(crate) async fn publish_port(
    engine: &EngineClient,
    device_id: &str,
    computer_id: String,
    port: u16,
    host_port: Option<u16>,
    scheme: Option<String>,
) -> Result<Value, EngineError> {
    let mut params = json!({"computerId": computer_id, "port": port});
    if let Some(host_port) = host_port {
        params["hostPort"] = json!(host_port);
    }
    if let Some(scheme) = scheme {
        params["scheme"] = Value::String(scheme);
    }
    engine
        .call("network.publish", with_device(params, Some(device_id)))
        .await
}

pub(crate) async fn unpublish_port(
    engine: &EngineClient,
    device_id: &str,
    computer_id: String,
    port: u16,
) -> Result<Value, EngineError> {
    engine
        .call(
            "network.unpublish",
            with_device(
                json!({"computerId": computer_id, "port": port}),
                Some(device_id),
            ),
        )
        .await
}

pub(crate) async fn open_port(
    engine: &EngineClient,
    device_id: &str,
    computer_id: String,
    port: u16,
    open: impl FnOnce(&Map<String, Value>, &str) -> Result<(), String>,
) -> Result<(), EngineError> {
    let opened = engine
        .call(
            "network.open",
            with_device(
                json!({"computerId": computer_id, "port": port}),
                Some(device_id),
            ),
        )
        .await?;
    let url = opened
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| EngineError::internal("The engine returned no address to open."))?;
    open_url(engine, url, open).await
}

pub(crate) async fn ssh_access_state(
    engine: &EngineClient,
    device_id: &str,
) -> Result<Value, EngineError> {
    engine
        .call("sshAccess.state", with_device(json!({}), Some(device_id)))
        .await
}

pub(crate) async fn save_ssh_access(
    engine: &EngineClient,
    device_id: &str,
    computer_id: String,
    enabled: bool,
    port: u16,
    bind_address: String,
    keys: Option<Vec<String>>,
) -> Result<Value, EngineError> {
    let mut params = json!({
        "computerId": computer_id, "enabled": enabled, "port": port, "bindAddress": bind_address,
    });
    if let Some(keys) = keys {
        params["keys"] = json!(keys);
    }
    engine
        .call("sshAccess.save", with_device(params, Some(device_id)))
        .await
}

#[tauri::command]
pub(crate) async fn remote_network_state(
    engine: Engine<'_>,
    device_id: String,
) -> Result<Value, EngineError> {
    network_state(&engine, &device_id).await
}

#[tauri::command]
pub(crate) async fn remote_save_network_port(
    engine: Engine<'_>,
    device_id: String,
    computer_id: String,
    port: u16,
    host_port: Option<u16>,
    scheme: Option<String>,
) -> Result<Value, EngineError> {
    publish_port(&engine, &device_id, computer_id, port, host_port, scheme).await
}

#[tauri::command]
pub(crate) async fn remote_remove_network_port(
    engine: Engine<'_>,
    device_id: String,
    computer_id: String,
    port: u16,
) -> Result<Value, EngineError> {
    unpublish_port(&engine, &device_id, computer_id, port).await
}

#[tauri::command]
pub(crate) async fn remote_open_network_port(
    engine: Engine<'_>,
    device_id: String,
    computer_id: String,
    port: u16,
) -> Result<(), EngineError> {
    open_port(&engine, &device_id, computer_id, port, browser).await
}

#[tauri::command]
pub(crate) async fn remote_ssh_access_state(
    engine: Engine<'_>,
    device_id: String,
) -> Result<Value, EngineError> {
    ssh_access_state(&engine, &device_id).await
}

#[tauri::command]
pub(crate) async fn remote_save_ssh_access(
    engine: Engine<'_>,
    device_id: String,
    computer_id: String,
    enabled: bool,
    port: u16,
    bind_address: String,
    keys: Option<Vec<String>>,
) -> Result<Value, EngineError> {
    save_ssh_access(
        &engine,
        &device_id,
        computer_id,
        enabled,
        port,
        bind_address,
        keys,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_settings_read_means_ask_for_new_remote_computers() {
        let failure = approval_from_settings(Err(EngineError::internal("settings unavailable")));
        assert_eq!(failure, Approval::Ask);
        let mut settings = Map::new();
        settings.insert("computerUseAutoApproval".into(), Value::Bool(true));
        assert_eq!(approval_from_settings(Ok(settings)), Approval::Auto);
    }

    #[test]
    fn notices_name_the_computer_by_its_owning_device() {
        let state = json!({"computers": [{"configuration": {"id": "c1", "name": "dev"}}]});
        assert_eq!(computer_name(&state, "c1").as_deref(), Some("dev"));
        assert_eq!(computer_name(&state, "c2"), None);
        assert_eq!(notice_computer("d1", "c1", "dev").id, "silo-remote:d1:c1");
    }
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};
    use std::sync::{Arc, Mutex};

    const DEVICE: &str = "00000000-0000-4000-8000-0000000000d1";
    const COMPUTER: &str = "00000000-0000-4000-8000-0000000000c1";

    fn state() -> Value {
        json!({"computers": [{"configuration": {"id": COMPUTER, "name": "dev"}}]})
    }

    fn replies(method: &str, _params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            "events.subscribe" => Ok(json!({"subscription": "sub"})),
            "computers.list" => Ok(json!({"linux": state(), "macos": {"computers": []}})),
            "computers.start"
            | "computers.stop"
            | "computers.restart"
            | "computers.delete"
            | "computers.configuration.change"
            | "checkpoints.create"
            | "checkpoints.fork"
            | "checkpoints.restore" => Ok(json!({"jobId": "job:1"})),
            "operations.job.get" => {
                Ok(json!({"jobId": "job:1", "state": "finished", "outcome": {"result": null}}))
            }
            "computers.dismissError" => Ok(state()),
            "connections.status" => Ok(json!({"enabled": true, "deviceId": DEVICE})),
            "connections.devices.list" => {
                Ok(json!([{"id": DEVICE, "name": "Office", "address": "office.local"}]))
            }
            "connections.devices.connect" => {
                Ok(json!({"id": DEVICE, "name": "Office", "address": "office.local"}))
            }
            "connections.setEnabled" => Ok(json!({"enabled": false, "deviceId": DEVICE})),
            "connections.ssh.authorizationCommand" | "connections.ssh.keySetupCommand" => {
                Ok(json!({"command": "ssh-copy-id office.local"}))
            }
            "network.open" => Ok(json!({"url": "http://127.0.0.1:3000"})),
            "settings.read" => Ok(json!({"settings": {"computerUseAutoApproval": true}})),
            _ => Ok(Value::Null),
        }
    }

    #[tokio::test]
    async fn connections_forward_to_their_canonical_methods() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(status(&engine).await.unwrap()["enabled"], true);
        assert_eq!(set_enabled(&engine, false).await.unwrap()["enabled"], false);
        assert_eq!(devices(&engine).await.unwrap()[0]["name"], "Office");
        connect(&engine, "office.local".into(), Some(true))
            .await
            .unwrap();
        remove(&engine, DEVICE.into()).await.unwrap();
        assert_eq!(
            fake.calls("connections.setEnabled"),
            vec![json!({"enabled": false})]
        );
        assert_eq!(
            fake.calls("connections.devices.connect"),
            vec![json!({"address": "office.local", "replace": true})]
        );
        assert_eq!(
            fake.calls("connections.devices.remove"),
            vec![json!({"deviceId": DEVICE})]
        );
    }

    #[tokio::test]
    async fn authorization_commands_are_run_in_this_devices_terminal() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let ran = Arc::new(Mutex::new(Vec::new()));
        for method in [
            "connections.ssh.authorizationCommand",
            "connections.ssh.keySetupCommand",
        ] {
            let record = ran.clone();
            run_ssh_command(
                &engine,
                method,
                "office.local".into(),
                move |settings, command| {
                    record.lock().unwrap().push((
                        settings.contains_key("computerUseAutoApproval"),
                        command.to_owned(),
                    ));
                    Ok(())
                },
            )
            .await
            .unwrap();
        }
        assert_eq!(ran.lock().unwrap().len(), 2);
        assert_eq!(
            ran.lock().unwrap()[0],
            (true, "ssh-copy-id office.local".to_owned())
        );
    }

    #[tokio::test]
    async fn a_device_snapshot_is_its_linux_state_and_actions_return_it_after_the_job() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let snapshot = snapshot(&engine, DEVICE, true).await.unwrap();
        assert_eq!(snapshot["computers"][0]["configuration"]["name"], "dev");
        let after = computer_action(&engine, DEVICE, COMPUTER, "start")
            .await
            .unwrap();
        assert_eq!(after, state());
        computer_action(&engine, DEVICE, COMPUTER, "dismiss-error")
            .await
            .unwrap();
        assert_eq!(
            fake.calls("computers.list"),
            vec![
                json!({"deviceId": DEVICE, "refreshRepositories": true}),
                json!({"deviceId": DEVICE}),
            ]
        );
        assert_eq!(
            fake.calls("computers.start"),
            vec![json!({"computerId": COMPUTER, "deviceId": DEVICE})]
        );
        assert_eq!(fake.calls("computers.dismissError").len(), 1);
        assert!(computer_action(&engine, DEVICE, COMPUTER, "explode")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn checkpoint_actions_run_the_matching_job_with_the_device() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        checkpoint_action(
            &engine,
            DEVICE,
            COMPUTER,
            "create",
            Some("c".into()),
            None,
            None,
        )
        .await
        .unwrap();
        checkpoint_action(
            &engine,
            DEVICE,
            COMPUTER,
            "fork",
            None,
            Some("k1".into()),
            Some("copy".into()),
        )
        .await
        .unwrap();
        checkpoint_action(
            &engine,
            DEVICE,
            COMPUTER,
            "restore",
            None,
            Some("k1".into()),
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            fake.calls("checkpoints.create"),
            vec![json!({"computerId": COMPUTER, "deviceId": DEVICE, "name": "c"})]
        );
        assert_eq!(
            fake.calls("checkpoints.fork"),
            vec![
                json!({"computerId": COMPUTER, "deviceId": DEVICE, "newName": "copy", "checkpointId": "k1"})
            ]
        );
        assert_eq!(
            fake.calls("checkpoints.restore"),
            vec![json!({"computerId": COMPUTER, "deviceId": DEVICE, "checkpointId": "k1"})]
        );
        assert!(
            checkpoint_action(&engine, DEVICE, COMPUTER, "restore", None, None, None)
                .await
                .is_err()
        );
        assert!(
            checkpoint_action(&engine, DEVICE, COMPUTER, "delete", None, None, None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_new_remote_computer_gets_this_devices_approval_preference() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let configuration = json!({"id": COMPUTER, "name": "dev"});
        upsert_computer(&engine, DEVICE, configuration.clone(), None)
            .await
            .unwrap();
        assert_eq!(
            fake.calls("computers.configuration.change"),
            vec![
                json!({"change": {"kind": "upsert", "configuration": configuration}, "deviceId": DEVICE})
            ]
        );
        assert_eq!(
            fake.calls("computerUse.approval"),
            vec![json!({"computerId": COMPUTER, "deviceId": DEVICE, "mode": "auto"})]
        );
        // Editing an existing computer leaves its approval alone.
        upsert_computer(&engine, DEVICE, configuration.clone(), Some(configuration))
            .await
            .unwrap();
        assert_eq!(fake.calls("computerUse.approval").len(), 1);
        delete_computer(&engine, DEVICE, COMPUTER, json!({"id": COMPUTER}))
            .await
            .unwrap();
        assert_eq!(
            fake.calls("computers.delete"),
            vec![json!({"computerId": COMPUTER, "deviceId": DEVICE, "expected": {"id": COMPUTER}})]
        );
    }

    #[tokio::test]
    async fn network_and_ssh_access_of_a_device_carry_its_id() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        network_state(&engine, DEVICE).await.unwrap();
        publish_port(&engine, DEVICE, COMPUTER.into(), 80, Some(8080), None)
            .await
            .unwrap();
        unpublish_port(&engine, DEVICE, COMPUTER.into(), 80)
            .await
            .unwrap();
        ssh_access_state(&engine, DEVICE).await.unwrap();
        save_ssh_access(
            &engine,
            DEVICE,
            COMPUTER.into(),
            true,
            2222,
            "127.0.0.1".into(),
            None,
        )
        .await
        .unwrap();
        let opened = Arc::new(Mutex::new(Vec::new()));
        let record = opened.clone();
        open_port(&engine, DEVICE, COMPUTER.into(), 3000, move |_, url| {
            record.lock().unwrap().push(url.to_owned());
            Ok(())
        })
        .await
        .unwrap();
        assert_eq!(
            fake.calls("network.state"),
            vec![json!({"deviceId": DEVICE})]
        );
        assert_eq!(
            fake.calls("network.publish"),
            vec![json!({"computerId": COMPUTER, "port": 80, "hostPort": 8080, "deviceId": DEVICE})]
        );
        assert_eq!(
            fake.calls("sshAccess.save"),
            vec![
                json!({"computerId": COMPUTER, "enabled": true, "port": 2222, "bindAddress": "127.0.0.1", "deviceId": DEVICE})
            ]
        );
        assert_eq!(
            *opened.lock().unwrap(),
            vec!["http://127.0.0.1:3000/".to_owned()]
        );
    }

    #[tokio::test]
    async fn a_device_the_engine_cannot_reach_is_the_typed_unavailable_state() {
        let fake = FakeEngine::start(|method, params| match method {
            "computers.list" | "network.state" | "sshAccess.state" => Err((
                "unsupported_remote_operation",
                "Remote devices are not available from this engine yet.".into(),
            )),
            _ => replies(method, params),
        });
        let (engine, _) = connected_client(&fake).await;
        for error in [
            snapshot(&engine, DEVICE, false).await.unwrap_err(),
            network_state(&engine, DEVICE).await.unwrap_err(),
            ssh_access_state(&engine, DEVICE).await.unwrap_err(),
        ] {
            assert_eq!(error.code, "unsupported_remote_operation");
        }
    }
}
