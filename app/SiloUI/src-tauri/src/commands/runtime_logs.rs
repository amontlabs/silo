//! Log queries: `silod` reads (and routes) the logs.
use crate::engine_client::{Engine, EngineClient, EngineError};
use serde_json::Value;

/// The UI serializes unset options as null; the engine's params leave them out.
pub(crate) fn without_nulls(request: Value) -> Value {
    match request {
        Value::Object(fields) => Value::Object(
            fields
                .into_iter()
                .filter(|(_, value)| !value.is_null())
                .collect(),
        ),
        other => other,
    }
}

pub(crate) async fn query(engine: &EngineClient, request: Value) -> Result<Value, EngineError> {
    engine.call("logs.query", without_nulls(request)).await
}

#[tauri::command]
pub(crate) async fn query_computer_logs(
    engine: Engine<'_>,
    request: Value,
) -> Result<Value, EngineError> {
    query(&engine, request).await
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};
    use serde_json::json;

    #[tokio::test]
    async fn a_query_is_forwarded_without_its_unset_options() {
        let fake = FakeEngine::start(|method, _| match method {
            "events.subscribe" => Ok(json!({"subscription": "sub"})),
            "logs.query" => Ok(json!({"entries": [], "totalMatches": 0})),
            _ => Ok(Value::Null),
        });
        let (engine, _) = connected_client(&fake).await;
        let page = query(
            &engine,
            json!({"computerId": "c1", "deviceId": null, "query": "boot", "cursor": null, "limit": 50}),
        )
        .await
        .unwrap();
        assert_eq!(page["totalMatches"], 0);
        assert_eq!(
            fake.calls("logs.query"),
            vec![json!({"computerId": "c1", "query": "boot", "limit": 50})]
        );
    }

    #[tokio::test]
    async fn an_unknown_computer_is_a_typed_error() {
        let fake = FakeEngine::start(|method, _| match method {
            "logs.query" => Err(("not_found", "This computer no longer exists.".into())),
            _ => Ok(json!({"subscription": "sub"})),
        });
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(
            query(&engine, json!({"computerId": "x"}))
                .await
                .unwrap_err()
                .code,
            "not_found"
        );
    }
}
