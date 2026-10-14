//! GitHub access, through `github.*`. The engine holds the credentials and the policy; this
//! device opens the authorization pages it hands back and shows the state. The UI names
//! computers by display name, the engine by id, so the state is translated both ways.

use std::{sync::Mutex, time::Duration};

use serde_json::{json, Map, Value};
use tauri::{Manager, WebviewWindow};

use crate::engine_client::{
    forwarding::{browser, open_url, roster, Roster},
    Engine, EngineClient, EngineError,
};

/// How often, and how many times, a starting connection is asked for its authorization page.
const URL_POLL: Duration = Duration::from_millis(250);
const URL_ATTEMPTS: u32 = 80;

/// The job of the connection in progress, which `cancel_github_connection` stops.
static ACTIVE_CONNECTION: Mutex<Option<String>> = Mutex::new(None);

fn require_main(window: &WebviewWindow) -> Result<(), EngineError> {
    require_main_label(window.label())
}

fn require_main_label(label: &str) -> Result<(), EngineError> {
    if label == "main" {
        Ok(())
    } else {
        Err(EngineError::internal(
            "Only the main window can manage GitHub access.",
        ))
    }
}

// MARK: Translation

fn rename_computer(entry: &mut Value, roster: &Roster) {
    if let Some(object) = entry.as_object_mut() {
        if let Some(id) = object.remove("computerId") {
            let name = roster.name_of(id.as_str().unwrap_or_default());
            object.insert("computer".to_owned(), Value::String(name));
        }
    }
}

/// The engine's state in the UI's shape: computers by display name, and a failed
/// computer operation always retryable with its diagnostics as text.
fn state_for_ui(mut state: Value, roster: &Roster) -> Value {
    for list in ["computers", "computerOperations"] {
        for entry in state
            .get_mut(list)
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
        {
            rename_computer(entry, roster);
        }
    }
    for operation in state
        .get_mut("computerOperations")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        let Some(object) = operation.as_object_mut() else {
            continue;
        };
        if object.get("status").and_then(Value::as_str) == Some("failed") {
            object.entry("canRetry").or_insert(Value::Bool(true));
        }
        match object.get("diagnosticDetails") {
            Some(Value::String(_)) | None => {}
            Some(Value::Null) => {
                object.remove("diagnosticDetails");
            }
            Some(other) => {
                let text = other.to_string();
                object.insert("diagnosticDetails".to_owned(), Value::String(text));
            }
        }
    }
    state
}

/// The UI's configuration in the engine's shape. The device identity is the engine's own.
fn configuration_for_engine(configuration: &Value, roster: &Roster) -> Result<Value, EngineError> {
    let policies = configuration
        .get("computers")
        .and_then(Value::as_array)
        .ok_or_else(|| EngineError::internal("Missing computer policies."))?;
    let computers = policies
        .iter()
        .map(|policy| {
            let name = policy
                .get("computer")
                .and_then(Value::as_str)
                .ok_or_else(|| EngineError::internal("A GitHub policy names no computer."))?;
            let field = |key: &str, default: Value| {
                policy
                    .get(key)
                    .filter(|value| !value.is_null())
                    .cloned()
                    .unwrap_or(default)
            };
            Ok(json!({
                "computerId": roster.id_of(name)?,
                "identity": field("identity", Value::Null),
                "repositoryMode": field("repositoryMode", json!("selected")),
                "repositories": field("repositories", json!([])),
                "allRepositoriesAllowChanges": field("allRepositoriesAllowChanges", json!(false)),
                "authenticationMethod": field("authenticationMethod", json!("oauth")),
            }))
        })
        .collect::<Result<Vec<_>, EngineError>>()?;
    Ok(json!({
        "computers": computers,
        "baseRevision": configuration.get("baseRevision").cloned().unwrap_or(Value::Null),
    }))
}

async fn view(engine: &EngineClient, state: Value) -> Result<Value, EngineError> {
    Ok(state_for_ui(state, &roster(engine, None).await?))
}

// MARK: Operations

pub(crate) async fn read_state(engine: &EngineClient) -> Result<Value, EngineError> {
    let state = engine.call("github.state", json!({})).await?;
    view(engine, state).await
}

/// A mutation that answers with the new state.
async fn mutate(engine: &EngineClient, method: &str, params: Value) -> Result<Value, EngineError> {
    let state = engine.call(method, params).await?;
    view(engine, state).await
}

fn remember_connection(job_id: Option<String>) {
    *ACTIVE_CONNECTION
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = job_id;
}

fn active_connection() -> Option<String> {
    ACTIVE_CONNECTION
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// Opens the authorization page the engine publishes for `job_id` as soon as it exists,
/// unless the job ends first.
async fn open_authorization(
    engine: &EngineClient,
    job_id: &str,
    open: &impl Fn(&Map<String, Value>, &str) -> Result<(), String>,
    poll: Duration,
) -> Result<(), EngineError> {
    for _ in 0..URL_ATTEMPTS {
        let state = engine.call("github.state", json!({})).await?;
        if let Some(url) = state.get("pendingAuthorizationUrl").and_then(Value::as_str) {
            return open_url(engine, url, open).await;
        }
        let job = engine
            .call("operations.job.get", json!({"jobId": job_id}))
            .await?;
        if job.get("state").and_then(Value::as_str) == Some("finished") {
            return Ok(());
        }
        tokio::time::sleep(poll).await;
    }
    Ok(())
}

/// Starts the connection, opens the page it asks for and waits for it to end.
pub(crate) async fn connect(
    engine: &EngineClient,
    open: impl Fn(&Map<String, Value>, &str) -> Result<(), String>,
    poll: Duration,
) -> Result<Value, EngineError> {
    let started = engine.call("github.connect", json!({})).await?;
    let job_id = started
        .get("jobId")
        .and_then(Value::as_str)
        .ok_or_else(|| EngineError::internal("The engine did not accept the connection."))?
        .to_owned();
    remember_connection(Some(job_id.clone()));
    let finished = async {
        if let Err(error) = open_authorization(engine, &job_id, &open, poll).await {
            let _ = engine
                .call("operations.cancel", json!({"jobId": job_id}))
                .await;
            let _ = engine.wait_job(&job_id).await;
            return Err(error);
        }
        engine.wait_job(&job_id).await
    }
    .await;
    if active_connection().as_deref() == Some(job_id.as_str()) {
        remember_connection(None);
    }
    finished?;
    read_state(engine).await
}

pub(crate) async fn cancel_connection(engine: &EngineClient) -> Result<Value, EngineError> {
    if let Some(job_id) = active_connection() {
        engine
            .call("operations.cancel", json!({"jobId": job_id}))
            .await?;
    }
    read_state(engine).await
}

/// Opens the page a command returned as `{url}`.
async fn open_returned_url(
    engine: &EngineClient,
    method: &str,
    open: impl FnOnce(&Map<String, Value>, &str) -> Result<(), String>,
) -> Result<(), EngineError> {
    let page = engine.call(method, json!({})).await?;
    let url = page
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| EngineError::internal("The engine returned no page to open."))?;
    open_url(engine, url, open).await
}

pub(crate) async fn save_configuration(
    engine: &EngineClient,
    configuration: &Value,
) -> Result<Value, EngineError> {
    let configuration = configuration_for_engine(configuration, &roster(engine, None).await?)?;
    mutate(
        engine,
        "github.configuration.save",
        json!({"configuration": configuration}),
    )
    .await
}

pub(crate) async fn retry_configuration(
    engine: &EngineClient,
    computer: Option<String>,
) -> Result<Value, EngineError> {
    let mut params = json!({});
    if let Some(name) = computer {
        params["computerId"] = Value::String(engine.computer_id(&name).await?);
    }
    mutate(engine, "github.configuration.retry", params).await
}

// MARK: Commands

#[tauri::command]
pub async fn read_github_state(engine: Engine<'_>) -> Result<Value, EngineError> {
    read_state(&engine).await
}

#[tauri::command]
pub async fn connect_github(
    engine: Engine<'_>,
    window: WebviewWindow,
) -> Result<Value, EngineError> {
    require_main(&window)?;
    let state = connect(&engine, browser, URL_POLL).await?;
    if let Some(main) = window.app_handle().get_webview_window("main") {
        let _ = main.show();
        let _ = main.set_focus();
    }
    Ok(state)
}

#[tauri::command]
pub async fn cancel_github_connection(
    engine: Engine<'_>,
    window: WebviewWindow,
) -> Result<Value, EngineError> {
    require_main(&window)?;
    cancel_connection(&engine).await
}

#[tauri::command]
pub async fn reopen_github_authorization(
    engine: Engine<'_>,
    window: WebviewWindow,
) -> Result<(), EngineError> {
    require_main(&window)?;
    open_returned_url(&engine, "github.connect.reopen", browser).await
}

#[tauri::command]
pub async fn manage_github_repositories(
    engine: Engine<'_>,
    window: WebviewWindow,
) -> Result<(), EngineError> {
    require_main(&window)?;
    open_returned_url(&engine, "github.repositories.manage", browser).await
}

#[tauri::command]
pub async fn refresh_github_repositories(
    engine: Engine<'_>,
    window: WebviewWindow,
) -> Result<Value, EngineError> {
    require_main(&window)?;
    mutate(&engine, "github.repositories.refresh", json!({})).await
}

#[tauri::command]
pub async fn disconnect_github(
    engine: Engine<'_>,
    window: WebviewWindow,
) -> Result<Value, EngineError> {
    require_main(&window)?;
    mutate(&engine, "github.disconnect", json!({})).await
}

#[tauri::command]
pub async fn set_github_access_enabled(
    engine: Engine<'_>,
    window: WebviewWindow,
    enabled: bool,
) -> Result<Value, EngineError> {
    require_main(&window)?;
    mutate(&engine, "github.access.set", json!({"enabled": enabled})).await
}

#[tauri::command]
pub async fn save_github_configuration(
    engine: Engine<'_>,
    window: WebviewWindow,
    configuration: Value,
) -> Result<Value, EngineError> {
    require_main(&window)?;
    save_configuration(&engine, &configuration).await
}

#[tauri::command]
pub async fn retry_github_configuration(
    engine: Engine<'_>,
    window: WebviewWindow,
    computer: Option<String>,
) -> Result<Value, EngineError> {
    require_main(&window)?;
    retry_configuration(&engine, computer).await
}

#[tauri::command]
pub async fn save_github_personal_token(
    engine: Engine<'_>,
    window: WebviewWindow,
    token: String,
) -> Result<Value, EngineError> {
    require_main(&window)?;
    mutate(
        &engine,
        "github.personalToken.save",
        json!({"token": token}),
    )
    .await
}

#[tauri::command]
pub async fn remove_github_personal_token(
    engine: Engine<'_>,
    window: WebviewWindow,
) -> Result<Value, EngineError> {
    require_main(&window)?;
    mutate(&engine, "github.personalToken.remove", json!({})).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_main_window_can_manage_github() {
        assert!(require_main_label("main").is_ok());
        for label in ["status", "other", ""] {
            assert!(require_main_label(label).is_err());
        }
    }
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};
    use std::sync::Arc;

    const DEV: &str = "00000000-0000-4000-8000-00000000000d";

    fn engine_state() -> Value {
        json!({
            "policyRevision": 3, "state": "connected", "account": "octo", "accessEnabled": true,
            "personalToken": {"state": "disconnected", "saved": false},
            "deviceIdentity": null, "repositoryCatalog": ["a/b"],
            "repositoryCatalogStatus": {"status": "available"},
            "computers": [{
                "computerId": DEV, "identity": {"name": "N", "email": "e@x", "apply": true},
                "repositoryMode": "selected", "repositories": [], "allRepositoriesAllowChanges": false,
                "authenticationMethod": "oauth"
            }],
            "computerOperations": [
                {"computerId": DEV, "status": "failed", "message": "no", "diagnosticDetails": {"a": 1}},
                {"computerId": "gone", "status": "succeeded", "message": "ok"}
            ],
            "pendingAuthorizationUrl": null
        })
    }

    fn replies(method: &str, _params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            "events.subscribe" => Ok(json!({"subscription": "sub"})),
            "computers.list" => Ok(json!({
                "linux": {"computers": [{"configuration": {"id": DEV, "name": "dev"}}]},
                "macos": {"computers": []}
            })),
            "github.state"
            | "github.disconnect"
            | "github.access.set"
            | "github.configuration.save"
            | "github.configuration.retry"
            | "github.personalToken.save"
            | "github.personalToken.remove"
            | "github.repositories.refresh" => Ok(engine_state()),
            "github.connect.reopen" | "github.repositories.manage" => {
                Ok(json!({"url": "https://github.com/login/oauth/authorize?x=1"}))
            }
            "settings.read" => Ok(json!({"settings": {}})),
            _ => Ok(Value::Null),
        }
    }

    #[tokio::test]
    async fn the_state_names_computers_and_keeps_the_ui_contract() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let state = read_state(&engine).await.unwrap();
        assert_eq!(state["computers"][0]["computer"], "dev");
        assert!(state["computers"][0].get("computerId").is_none());
        let failed = &state["computerOperations"][0];
        assert_eq!(failed["computer"], "dev");
        assert_eq!(failed["canRetry"], true);
        assert_eq!(failed["diagnosticDetails"], "{\"a\":1}");
        assert_eq!(state["computerOperations"][1]["computer"], "gone");
        assert_eq!(state["policyRevision"], 3);
    }

    #[tokio::test]
    async fn saving_a_configuration_sends_ids_and_defaults() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        save_configuration(
            &engine,
            &json!({
                "baseRevision": 3, "deviceIdentity": null,
                "computers": [{
                    "computer": "dev", "identity": {"name": "N", "email": "e@x", "apply": true},
                    "authenticationMethod": null, "repositories": [{"repository": "a/b", "allowPushes": true}]
                }]
            }),
        )
        .await
        .unwrap();
        assert_eq!(
            fake.calls("github.configuration.save"),
            vec![json!({"configuration": {
                "baseRevision": 3,
                "computers": [{
                    "computerId": DEV, "identity": {"name": "N", "email": "e@x", "apply": true},
                    "repositoryMode": "selected",
                    "repositories": [{"repository": "a/b", "allowPushes": true}],
                    "allRepositoriesAllowChanges": false, "authenticationMethod": "oauth"
                }]
            }})]
        );
        let unknown = save_configuration(&engine, &json!({"computers": [{"computer": "ghost"}]}))
            .await
            .unwrap_err();
        assert_eq!(unknown.code, "not_found");
        assert_eq!(fake.calls("github.configuration.save").len(), 1);
    }

    #[tokio::test]
    async fn retry_and_the_simple_mutations_forward_their_params() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        retry_configuration(&engine, Some("dev".into()))
            .await
            .unwrap();
        retry_configuration(&engine, None).await.unwrap();
        mutate(&engine, "github.access.set", json!({"enabled": false}))
            .await
            .unwrap();
        assert_eq!(
            fake.calls("github.configuration.retry"),
            vec![json!({"computerId": DEV}), json!({})]
        );
        assert_eq!(
            fake.calls("github.access.set"),
            vec![json!({"enabled": false})]
        );
    }

    #[tokio::test]
    async fn connecting_opens_the_published_page_waits_for_the_job_and_returns_the_state() {
        let pending = Arc::new(Mutex::new(false));
        let handler_pending = pending.clone();
        let fake = FakeEngine::start(move |method, params| match method {
            "github.connect" => {
                *handler_pending.lock().unwrap() = true;
                Ok(json!({"jobId": "job:7"}))
            }
            "github.state" if *handler_pending.lock().unwrap() => {
                let mut state = engine_state();
                state["pendingAuthorizationUrl"] =
                    json!("https://github.com/login/oauth/authorize?x=1");
                Ok(state)
            }
            "operations.job.get" => {
                Ok(json!({"jobId": "job:7", "state": "finished", "outcome": {"result": null}}))
            }
            _ => replies(method, params),
        });
        let (engine, _) = connected_client(&fake).await;
        let opened = Arc::new(Mutex::new(Vec::new()));
        let record = opened.clone();
        let state = connect(
            &engine,
            move |_, url| {
                record.lock().unwrap().push(url.to_owned());
                Ok(())
            },
            Duration::from_millis(5),
        )
        .await
        .unwrap();
        assert_eq!(state["account"], "octo");
        assert_eq!(
            *opened.lock().unwrap(),
            vec!["https://github.com/login/oauth/authorize?x=1".to_owned()]
        );
        assert_eq!(
            fake.calls("operations.job.acknowledge"),
            vec![json!({"jobId": "job:7"})]
        );
    }

    #[tokio::test]
    async fn a_connection_that_fails_to_start_or_ends_in_error_is_the_engines_error() {
        let fake = FakeEngine::start(|method, params| match method {
            "github.connect" => Ok(json!({"jobId": "job:8"})),
            "operations.job.get" => Ok(json!({
                "jobId": "job:8", "state": "finished",
                "outcome": {"error": {"code": "cancelled", "message": "GitHub connection cancelled."}}
            })),
            _ => replies(method, params),
        });
        let (engine, _) = connected_client(&fake).await;
        let error = connect(&engine, |_, _| Ok(()), Duration::from_millis(5))
            .await
            .unwrap_err();
        assert_eq!(error.code, "cancelled");
        assert_eq!(error.message, "GitHub connection cancelled.");

        let refused = FakeEngine::start(|method, params| match method {
            "github.connect" => Err(("unsupported_remote_operation", "No.".into())),
            _ => replies(method, params),
        });
        let (engine, _) = connected_client(&refused).await;
        let error = connect(&engine, |_, _| Ok(()), Duration::from_millis(5))
            .await
            .unwrap_err();
        assert_eq!(error.code, "unsupported_remote_operation");
    }

    #[tokio::test]
    async fn authorization_pages_the_engine_returns_are_opened_here() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let opened = Arc::new(Mutex::new(Vec::new()));
        for method in ["github.connect.reopen", "github.repositories.manage"] {
            let record = opened.clone();
            open_returned_url(&engine, method, move |_, url| {
                record.lock().unwrap().push(url.to_owned());
                Ok(())
            })
            .await
            .unwrap();
        }
        assert_eq!(opened.lock().unwrap().len(), 2);
    }
}
