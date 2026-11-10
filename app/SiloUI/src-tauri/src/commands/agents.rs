//! Tauri adapters for the agents a computer has: what is installed, and installing one.
//! Both forward to `silod`; an install is a job whose steps reach the window as
//! `silo://agent-install-progress`.
use crate::engine_client::{targets, Engine, EngineClient, EngineError};
use serde_json::Value;
use silo_api::methods::{
    agents::{AgentId, AgentsInstallParams},
    requests,
};

/// The agents of `computer` (a local name or a remote target) with their states.
pub(crate) async fn list(engine: &EngineClient, computer: &str) -> Result<Value, EngineError> {
    let target = targets::resolve(engine, computer).await?;
    engine.call(requests::agents_list(target.computer())).await
}

/// Installs `agent` in the running `computer` and returns the computer's agents afterwards.
pub(crate) async fn install(
    engine: &EngineClient,
    computer: &str,
    agent: AgentId,
) -> Result<Value, EngineError> {
    let target = targets::resolve(engine, computer).await?;
    engine
        .run_job(requests::agents_install(AgentsInstallParams {
            computer_id: target.computer_id,
            device_id: target.device_id,
            agent,
        }))
        .await
}

#[tauri::command]
pub(crate) async fn list_agents(engine: Engine<'_>, computer: String) -> Result<Value, String> {
    list(&engine, &computer).await.map_err(String::from)
}

#[tauri::command]
pub(crate) async fn install_agent(
    engine: Engine<'_>,
    computer: String,
    agent: AgentId,
) -> Result<Value, String> {
    install(&engine, &computer, agent)
        .await
        .map_err(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};
    use serde_json::json;
    use silo_api::methods::MethodName;

    const DEV: &str = "00000000-0000-4000-8000-0000000000aa";

    fn replies(method: MethodName, _params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            MethodName::EventsSubscribe => Ok(json!({"subscription": "sub"})),
            MethodName::ComputersList => Ok(json!({"linux": {"computers": [
                {"configuration": {"id": DEV, "name": "dev"}}
            ]}})),
            MethodName::AgentsList => Ok(json!({"agents": []})),
            MethodName::AgentsInstall => Ok(json!({"jobId": "job:1"})),
            MethodName::OperationsJobGet => Ok(json!({"jobId": "job:1", "state": "finished",
                "outcome": {"result": {"agents": []}}})),
            _ => Ok(Value::Null),
        }
    }

    #[tokio::test]
    async fn a_local_computer_is_listed_and_installed_by_its_id() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        list(&engine, "dev").await.unwrap();
        install(&engine, "dev", AgentId::Codex).await.unwrap();
        assert_eq!(
            fake.calls(MethodName::AgentsList),
            vec![json!({"computerId": DEV})]
        );
        assert_eq!(
            fake.calls(MethodName::AgentsInstall),
            vec![json!({"computerId": DEV, "agent": "codex"})]
        );
    }

    #[tokio::test]
    async fn a_computer_on_another_device_carries_its_device() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        list(&engine, "silo-remote:office:c9").await.unwrap();
        install(&engine, "silo-remote:office:c9", AgentId::ClaudeCode)
            .await
            .unwrap();
        assert_eq!(
            fake.calls(MethodName::AgentsList),
            vec![json!({"computerId": "c9", "deviceId": "office"})]
        );
        assert_eq!(
            fake.calls(MethodName::AgentsInstall),
            vec![json!({"computerId": "c9", "deviceId": "office", "agent": "claude-code"})]
        );
    }

    #[tokio::test]
    async fn a_computer_that_does_not_exist_is_not_found() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(list(&engine, "ghost").await.unwrap_err().code, "not_found");
        assert!(fake.calls(MethodName::AgentsList).is_empty());
    }
}
