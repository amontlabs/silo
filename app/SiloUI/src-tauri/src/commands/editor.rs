use crate::engine_client::{Engine, EngineClient, EngineError};
use serde_json::json;
use tauri::WebviewWindow;

pub(crate) async fn include_notice(engine: &EngineClient) -> Result<Option<String>, EngineError> {
    let notice = engine.call("editor.includeNotice", json!({})).await?;
    Ok(notice.as_str().map(str::to_owned))
}

#[tauri::command]
pub(crate) async fn read_editor_include_notice(
    engine: Engine<'_>,
    window: WebviewWindow,
) -> Result<Option<String>, EngineError> {
    if window.label() != "main" {
        return Err(EngineError::internal(
            "Only the main window can read the editor connection notice.",
        ));
    }
    include_notice(&engine).await
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};
    use serde_json::Value;

    #[tokio::test]
    async fn the_include_notice_is_the_engines_text_or_nothing() {
        let notice =
            std::sync::Arc::new(std::sync::Mutex::new(json!("Add Include to ~/.ssh/config")));
        let current = notice.clone();
        let fake = FakeEngine::start(move |method, _| match method {
            "events.subscribe" => Ok(json!({"subscription": "sub"})),
            "editor.includeNotice" => Ok(current.lock().unwrap().clone()),
            "engine.state" => Ok(Value::Null),
            _ => Ok(Value::Null),
        });
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(
            include_notice(&engine).await.unwrap().as_deref(),
            Some("Add Include to ~/.ssh/config")
        );
        *notice.lock().unwrap() = Value::Null;
        assert_eq!(include_notice(&engine).await.unwrap(), None);
    }

    #[tokio::test]
    async fn an_unavailable_engine_method_is_typed() {
        let fake = FakeEngine::start(|method, _| match method {
            "editor.includeNotice" => Err(("unsupported_remote_operation", "Not here.".into())),
            _ => Ok(json!({"subscription": "sub"})),
        });
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(
            include_notice(&engine).await.unwrap_err().code,
            "unsupported_remote_operation"
        );
    }
}
