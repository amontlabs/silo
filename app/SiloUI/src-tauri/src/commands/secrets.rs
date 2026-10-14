//! Secrets of this device, through `secrets.*`. The UI names computers by display name; the
//! engine by id, so both directions are translated here. Values are write-only.

use serde_json::{json, Value};
use tauri::WebviewWindow;

use crate::engine_client::{
    forwarding::{roster, Roster},
    Engine, EngineClient, EngineError,
};

fn require_main(window: &WebviewWindow) -> Result<(), EngineError> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(EngineError::internal(
            "Secret changes are only available in the main window.",
        ))
    }
}

fn names(ids: &Value, roster: &Roster) -> Value {
    Value::Array(
        ids.as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|id| Value::String(roster.name_of(id)))
            .collect(),
    )
}

/// One engine secret in the UI's shape: computers by display name.
fn secret_for_ui(mut secret: Value, roster: &Roster) -> Value {
    if let Some(object) = secret.as_object_mut() {
        if let Some(ids) = object.remove("computerIds") {
            object.insert("computers".to_owned(), names(&ids, roster));
        }
        if let Some(ids) = object.remove("pendingComputerIds") {
            object.insert("pendingComputers".to_owned(), names(&ids, roster));
        }
    }
    secret
}

async fn secrets_for_ui(engine: &EngineClient, listed: Value) -> Result<Vec<Value>, EngineError> {
    let roster = roster(engine, None).await?;
    Ok(listed
        .as_array()
        .into_iter()
        .flatten()
        .cloned()
        .map(|secret| secret_for_ui(secret, &roster))
        .collect())
}

/// The UI's save request in the engine's shape.
fn request_for_engine(request: &Value, roster: &Roster) -> Result<Value, EngineError> {
    let text = |key: &str| request.get(key).filter(|value| !value.is_null()).cloned();
    let computer_ids = request
        .get("computers")
        .and_then(Value::as_array)
        .ok_or_else(|| EngineError::internal("Choose the computers for this secret."))?
        .iter()
        .map(|name| {
            roster
                .id_of(name.as_str().unwrap_or_default())
                .map(Value::String)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut engine_request = json!({
        "operation": text("operation").ok_or_else(|| EngineError::internal("Missing secret operation."))?,
        "name": text("name").ok_or_else(|| EngineError::internal("Missing secret name."))?,
        "computerIds": computer_ids,
        "allowedDomains": request.get("allowedDomains").cloned().unwrap_or_else(|| json!([])),
    });
    for key in ["id", "value"] {
        if let Some(value) = text(key) {
            engine_request[key] = value;
        }
    }
    Ok(engine_request)
}

pub(crate) async fn list(engine: &EngineClient) -> Result<Vec<Value>, EngineError> {
    let listed = engine.call("secrets.list", json!({})).await?;
    secrets_for_ui(engine, listed).await
}

pub(crate) async fn save(engine: &EngineClient, request: Value) -> Result<Vec<Value>, EngineError> {
    let request = request_for_engine(&request, &roster(engine, None).await?)?;
    let saved = engine
        .call("secrets.save", json!({"request": request}))
        .await?;
    secrets_for_ui(engine, saved).await
}

async fn change(
    engine: &EngineClient,
    method: &str,
    id: String,
) -> Result<Vec<Value>, EngineError> {
    let changed = engine.call(method, json!({"id": id})).await?;
    secrets_for_ui(engine, changed).await
}

#[tauri::command]
pub async fn read_secrets_state(engine: Engine<'_>) -> Result<Vec<Value>, EngineError> {
    list(&engine).await
}

#[tauri::command]
pub async fn save_secret(
    engine: Engine<'_>,
    window: WebviewWindow,
    request: Value,
) -> Result<Vec<Value>, EngineError> {
    require_main(&window)?;
    save(&engine, request).await
}

#[tauri::command]
pub async fn remove_secret(
    engine: Engine<'_>,
    window: WebviewWindow,
    id: String,
) -> Result<Vec<Value>, EngineError> {
    require_main(&window)?;
    change(&engine, "secrets.remove", id).await
}

#[tauri::command]
pub async fn retry_secret(
    engine: Engine<'_>,
    window: WebviewWindow,
    id: String,
) -> Result<Vec<Value>, EngineError> {
    require_main(&window)?;
    change(&engine, "secrets.retry", id).await
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};

    const DEV: &str = "00000000-0000-4000-8000-00000000000d";

    fn engine_secret() -> Value {
        json!({
            "id": "s1", "name": "TOKEN", "computerIds": [DEV], "allowedDomains": ["example.com"],
            "state": "applying", "pendingComputerIds": [DEV], "removing": false, "error": null
        })
    }

    fn replies(method: &str, _params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            "events.subscribe" => Ok(json!({"subscription": "sub"})),
            "computers.list" => Ok(json!({
                "linux": {"computers": [{"configuration": {"id": DEV, "name": "dev"}}]},
                "macos": {"computers": []}
            })),
            "secrets.list" | "secrets.save" | "secrets.remove" | "secrets.retry" => {
                Ok(json!([engine_secret()]))
            }
            _ => Ok(Value::Null),
        }
    }

    #[tokio::test]
    async fn secrets_are_listed_with_computer_names() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let secrets = list(&engine).await.unwrap();
        assert_eq!(secrets[0]["computers"], json!(["dev"]));
        assert_eq!(secrets[0]["pendingComputers"], json!(["dev"]));
        assert!(secrets[0].get("computerIds").is_none());
        assert_eq!(secrets[0]["state"], "applying");
    }

    #[tokio::test]
    async fn saving_translates_names_to_ids_and_keeps_the_value_write_only() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let saved = save(
            &engine,
            json!({"operation": "add", "id": null, "name": "TOKEN", "value": "v",
                   "computers": ["dev"], "allowedDomains": ["example.com"]}),
        )
        .await
        .unwrap();
        assert_eq!(saved[0]["computers"], json!(["dev"]));
        assert_eq!(
            fake.calls("secrets.save"),
            vec![json!({"request": {
                "operation": "add", "name": "TOKEN", "value": "v",
                "computerIds": [DEV], "allowedDomains": ["example.com"]
            }})]
        );
        let unknown = save(
            &engine,
            json!({"operation": "add", "name": "T", "computers": ["ghost"], "allowedDomains": []}),
        )
        .await
        .unwrap_err();
        assert_eq!(unknown.code, "not_found");
        assert_eq!(fake.calls("secrets.save").len(), 1);
    }

    #[tokio::test]
    async fn remove_and_retry_forward_the_id_and_errors_stay_typed() {
        let fake = FakeEngine::start(|method, params| match method {
            "secrets.retry" => Err(("busy", "Another change is running.".into())),
            _ => replies(method, params),
        });
        let (engine, _) = connected_client(&fake).await;
        change(&engine, "secrets.remove", "s1".into())
            .await
            .unwrap();
        assert_eq!(fake.calls("secrets.remove"), vec![json!({"id": "s1"})]);
        let error = change(&engine, "secrets.retry", "s1".into())
            .await
            .unwrap_err();
        assert_eq!(error.code, "busy");
    }
}
