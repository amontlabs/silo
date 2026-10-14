//! Published ports of this device's computers, through `network.*`. Opening the URL in the
//! browser chosen in settings stays on this device.

use serde_json::{json, Value};

use crate::engine_client::{
    forwarding::{browser, open_url},
    Engine, EngineClient, EngineError,
};

pub(crate) async fn state(engine: &EngineClient) -> Result<Value, EngineError> {
    engine.call("network.state", json!({})).await
}

pub(crate) async fn save_port(
    engine: &EngineClient,
    computer: &str,
    port: u16,
    host_port: Option<u16>,
    scheme: Option<String>,
) -> Result<Value, EngineError> {
    let mut params = json!({"computerId": engine.computer_id(computer).await?, "port": port});
    if let Some(host_port) = host_port {
        params["hostPort"] = json!(host_port);
    }
    if let Some(scheme) = scheme {
        params["scheme"] = Value::String(scheme);
    }
    engine.call("network.publish", params).await
}

pub(crate) async fn remove_port(
    engine: &EngineClient,
    computer: &str,
    port: u16,
) -> Result<Value, EngineError> {
    let params = json!({"computerId": engine.computer_id(computer).await?, "port": port});
    engine.call("network.unpublish", params).await
}

pub(crate) async fn open_port(
    engine: &EngineClient,
    computer: &str,
    port: u16,
    open: impl FnOnce(&serde_json::Map<String, Value>, &str) -> Result<(), String>,
) -> Result<(), EngineError> {
    let params = json!({"computerId": engine.computer_id(computer).await?, "port": port});
    let opened = engine.call("network.open", params).await?;
    let url = opened
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| EngineError::internal("The engine returned no address to open."))?;
    open_url(engine, url, open).await
}

#[tauri::command]
pub(crate) async fn read_network_state(engine: Engine<'_>) -> Result<Value, EngineError> {
    state(&engine).await
}

#[tauri::command]
pub(crate) async fn save_network_port(
    engine: Engine<'_>,
    computer: String,
    port: u16,
    host_port: Option<u16>,
    scheme: Option<String>,
) -> Result<Value, EngineError> {
    save_port(&engine, &computer, port, host_port, scheme).await
}

#[tauri::command]
pub(crate) async fn remove_network_port(
    engine: Engine<'_>,
    computer: String,
    port: u16,
) -> Result<Value, EngineError> {
    remove_port(&engine, &computer, port).await
}

#[tauri::command]
pub(crate) async fn open_network_port(
    engine: Engine<'_>,
    computer: String,
    port: u16,
) -> Result<(), EngineError> {
    open_port(&engine, &computer, port, browser).await
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};
    use std::sync::{Arc, Mutex};

    const DEV: &str = "00000000-0000-4000-8000-00000000000d";

    fn replies(method: &str, _params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            "events.subscribe" => Ok(json!({"subscription": "sub"})),
            "computers.list" => Ok(json!({
                "linux": {"computers": [{"configuration": {"id": DEV, "name": "dev"}}]},
                "macos": {"computers": []}
            })),
            "network.state" | "network.publish" | "network.unpublish" => {
                Ok(json!({"computers": [{"computer": "dev", "ports": []}]}))
            }
            "network.open" => Ok(json!({"url": "http://127.0.0.1:8080"})),
            "settings.read" => Ok(json!({"settings": {"browserApplication": "x"}})),
            _ => Ok(Value::Null),
        }
    }

    #[tokio::test]
    async fn ports_are_read_published_and_removed_by_computer_id() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(
            state(&engine).await.unwrap()["computers"][0]["computer"],
            "dev"
        );
        save_port(&engine, "dev", 8080, Some(9090), Some("https".into()))
            .await
            .unwrap();
        save_port(&engine, "dev", 80, None, None).await.unwrap();
        remove_port(&engine, "dev", 8080).await.unwrap();
        assert_eq!(
            fake.calls("network.publish"),
            vec![
                json!({"computerId": DEV, "port": 8080, "hostPort": 9090, "scheme": "https"}),
                json!({"computerId": DEV, "port": 80}),
            ]
        );
        assert_eq!(
            fake.calls("network.unpublish"),
            vec![json!({"computerId": DEV, "port": 8080})]
        );
    }

    #[tokio::test]
    async fn opening_a_port_launches_the_returned_url_with_this_devices_settings() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let opened = Arc::new(Mutex::new(Vec::new()));
        let record = opened.clone();
        open_port(&engine, "dev", 8080, move |settings, url| {
            record
                .lock()
                .unwrap()
                .push((settings.get("browserApplication").cloned(), url.to_owned()));
            Ok(())
        })
        .await
        .unwrap();
        assert_eq!(
            *opened.lock().unwrap(),
            vec![(Some(json!("x")), "http://127.0.0.1:8080/".to_owned())]
        );
        assert_eq!(
            fake.calls("network.open"),
            vec![json!({"computerId": DEV, "port": 8080})]
        );
    }

    #[tokio::test]
    async fn errors_and_unknown_computers_pass_through_typed() {
        let fake = FakeEngine::start(|method, params| match method {
            "network.state" => Err(("unsupported_remote_operation", "Not on this device.".into())),
            _ => replies(method, params),
        });
        let (engine, _) = connected_client(&fake).await;
        let error = state(&engine).await.unwrap_err();
        assert_eq!(error.code, "unsupported_remote_operation");
        assert_eq!(error.message, "Not on this device.");
        assert_eq!(
            remove_port(&engine, "ghost", 1).await.unwrap_err().code,
            "not_found"
        );
        assert!(fake.calls("network.unpublish").is_empty());
    }
}
