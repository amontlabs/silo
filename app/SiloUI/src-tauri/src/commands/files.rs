//! Folder listings of the computers, through `files.list`.

use serde_json::{json, Value};

use crate::engine_client::{
    forwarding::{resolve, with_device},
    Engine, EngineClient, EngineError,
};

pub(crate) async fn list_directory(
    engine: &EngineClient,
    computer: &str,
    path: String,
    offset: usize,
    snapshot_id: Option<String>,
) -> Result<Value, EngineError> {
    let (computer_id, device_id) = resolve(engine, computer).await?;
    let mut params = json!({"computerId": computer_id, "path": path, "offset": offset});
    if let Some(snapshot_id) = snapshot_id {
        params["snapshotId"] = Value::String(snapshot_id);
    }
    engine
        .call("files.list", with_device(params, device_id.as_deref()))
        .await
}

#[tauri::command]
pub(crate) async fn list_computer_directory(
    engine: Engine<'_>,
    computer: String,
    path: String,
    offset: usize,
    snapshot_id: Option<String>,
) -> Result<Value, EngineError> {
    list_directory(&engine, &computer, path, offset, snapshot_id).await
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};

    const DEV: &str = "00000000-0000-4000-8000-00000000000d";
    const DEVICE: &str = "00000000-0000-4000-8000-0000000000d1";

    fn replies(method: &str, _params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            "events.subscribe" => Ok(json!({"subscription": "sub"})),
            "computers.list" => Ok(json!({
                "linux": {"computers": [{"configuration": {"id": DEV, "name": "dev"}}]},
                "macos": {"computers": []}
            })),
            "files.list" => Ok(json!({"entries": [], "nextOffset": null, "snapshotId": "s1"})),
            _ => Ok(Value::Null),
        }
    }

    #[tokio::test]
    async fn a_local_name_and_a_remote_target_list_through_the_same_method() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let page = list_directory(&engine, "dev", "/workspace".into(), 0, None)
            .await
            .unwrap();
        assert_eq!(page["snapshotId"], "s1");
        list_directory(
            &engine,
            &format!("silo-remote:{DEVICE}:{DEV}"),
            "/workspace/a".into(),
            200,
            Some("s1".into()),
        )
        .await
        .unwrap();
        assert_eq!(
            fake.calls("files.list"),
            vec![
                json!({"computerId": DEV, "path": "/workspace", "offset": 0}),
                json!({"computerId": DEV, "deviceId": DEVICE, "path": "/workspace/a", "offset": 200, "snapshotId": "s1"}),
            ]
        );
    }

    #[tokio::test]
    async fn listing_failures_are_typed_and_unknown_computers_never_reach_the_engine() {
        let fake = FakeEngine::start(|method, params| match method {
            "files.list" => Err(("not_found", "That folder no longer exists.".into())),
            _ => replies(method, params),
        });
        let (engine, _) = connected_client(&fake).await;
        let error = list_directory(&engine, "dev", "/workspace/x".into(), 0, None)
            .await
            .unwrap_err();
        assert_eq!(error.code, "not_found");
        assert!(
            list_directory(&engine, "ghost", "/workspace".into(), 0, None)
                .await
                .is_err()
        );
        assert_eq!(fake.calls("files.list").len(), 1);
    }
}
