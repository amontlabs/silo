//! Folder listings of the computers, through `files.list`.

use serde_json::Value;
use silo_api::methods::{files::FileListParams, requests};

use crate::engine_client::{forwarding::resolve, Engine, EngineClient, EngineError};

pub(crate) async fn list_directory(
    engine: &EngineClient,
    computer: &str,
    path: String,
    offset: usize,
    snapshot_id: Option<String>,
) -> Result<Value, EngineError> {
    let (computer_id, device_id) = resolve(engine, computer).await?;
    engine
        .call(requests::files_list(FileListParams {
            computer_id,
            device_id,
            path,
            offset: offset as u64,
            snapshot_id,
        }))
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
    use serde_json::json;
    use silo_api::methods::MethodName;

    const DEV: &str = "00000000-0000-4000-8000-00000000000d";
    const DEVICE: &str = "00000000-0000-4000-8000-0000000000d1";

    fn replies(method: MethodName, _params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            MethodName::EventsSubscribe => Ok(json!({"subscription": "sub"})),
            MethodName::ComputersList => Ok(json!({
                "linux": {"computers": [{"configuration": {"id": DEV, "name": "dev"}}]},
                "macos": {"computers": []}
            })),
            MethodName::FilesList => {
                Ok(json!({"entries": [], "nextOffset": null, "snapshotId": "s1"}))
            }
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
            fake.calls(MethodName::FilesList),
            vec![
                json!({"computerId": DEV, "path": "/workspace", "offset": 0}),
                json!({"computerId": DEV, "deviceId": DEVICE, "path": "/workspace/a", "offset": 200, "snapshotId": "s1"}),
            ]
        );
    }

    #[tokio::test]
    async fn listing_failures_are_typed_and_unknown_computers_never_reach_the_engine() {
        let fake = FakeEngine::start(|method, params| match method {
            MethodName::FilesList => Err(("not_found", "That folder no longer exists.".into())),
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
        assert_eq!(fake.calls(MethodName::FilesList).len(), 1);
    }
}
