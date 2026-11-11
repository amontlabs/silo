//! A computer's network policy (`network.policy.get`, `network.policy.set`) and whether it has
//! Silo's tools (`guestClient.*`). The policy belongs to the device that runs the computer, so
//! both address a computer of this device by its name.
use serde_json::Value;
use silo_api::methods::{
    guest_client::GuestClientComputerParams,
    network::{NetworkPolicy, NetworkPolicySetParams},
    requests,
};

use crate::engine_client::{
    params::{decode, target},
    Engine, EngineClient, EngineError,
};

/// The policy of `computer`: its mode, hosts and presets, what Silo adds on its own, the presets
/// with an update waiting, and whether a change applies now or at the next start.
pub(crate) async fn read_policy(
    engine: &EngineClient,
    computer: &str,
) -> Result<Value, EngineError> {
    let computer_id = engine.computer_id(computer).await?;
    engine
        .call(requests::network_policy_get(target(&computer_id, None)))
        .await
}

/// Saves `policy` for `computer`. `expected` is the policy the person was looking at: the engine
/// saves nothing when the computer's policy has changed since.
pub(crate) async fn save_policy(
    engine: &EngineClient,
    computer: &str,
    policy: Value,
    expected: Option<Value>,
) -> Result<Value, EngineError> {
    let policy: NetworkPolicy = decode(policy, "network policy")?;
    let expected: Option<NetworkPolicy> = expected
        .map(|expected| decode(expected, "network policy"))
        .transpose()?;
    engine
        .call(requests::network_policy_set(NetworkPolicySetParams {
            computer_id: engine.computer_id(computer).await?,
            device_id: None,
            policy,
            expected,
        }))
        .await
}

async fn tools_params(
    engine: &EngineClient,
    computer: &str,
) -> Result<GuestClientComputerParams, EngineError> {
    Ok(GuestClientComputerParams {
        computer_id: engine.computer_id(computer).await?,
    })
}

/// Whether `computer` has Silo's tools, and when their network rule takes effect.
pub(crate) async fn read_tools(
    engine: &EngineClient,
    computer: &str,
) -> Result<Value, EngineError> {
    engine
        .call(requests::guest_client_status(
            tools_params(engine, computer).await?,
        ))
        .await
}

/// Turns Silo's tools on in `computer`.
pub(crate) async fn enable_tools(
    engine: &EngineClient,
    computer: &str,
) -> Result<Value, EngineError> {
    engine
        .call(requests::guest_client_enable(
            tools_params(engine, computer).await?,
        ))
        .await
}

/// Turns Silo's tools off in `computer`.
pub(crate) async fn revoke_tools(
    engine: &EngineClient,
    computer: &str,
) -> Result<Value, EngineError> {
    engine
        .call(requests::guest_client_revoke(
            tools_params(engine, computer).await?,
        ))
        .await
}

#[tauri::command]
pub(crate) async fn read_network_policy(
    engine: Engine<'_>,
    computer: String,
) -> Result<Value, String> {
    read_policy(&engine, &computer).await.map_err(String::from)
}

#[tauri::command]
pub(crate) async fn save_network_policy(
    engine: Engine<'_>,
    computer: String,
    policy: Value,
    expected: Option<Value>,
) -> Result<Value, String> {
    save_policy(&engine, &computer, policy, expected)
        .await
        .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn read_computer_tools(
    engine: Engine<'_>,
    computer: String,
) -> Result<Value, String> {
    read_tools(&engine, &computer).await.map_err(String::from)
}

#[tauri::command]
pub(crate) async fn enable_computer_tools(
    engine: Engine<'_>,
    computer: String,
) -> Result<Value, String> {
    enable_tools(&engine, &computer).await.map_err(String::from)
}

#[tauri::command]
pub(crate) async fn revoke_computer_tools(
    engine: Engine<'_>,
    computer: String,
) -> Result<Value, String> {
    revoke_tools(&engine, &computer).await.map_err(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};
    use serde_json::json;
    use silo_api::methods::{guest_client::GuestClientStatus, network, MethodName};

    const DEV: &str = "00000000-0000-4000-8000-0000000000bb";

    fn policy_state(mode: &str) -> Value {
        json!({
            "computerId": DEV,
            "policy": {"mode": mode, "hosts": ["example.com"], "presets": ["npm"]},
            "allowedHosts": [{"host": "registry.npmjs.org", "ports": [443], "from": "preset:npm"}],
            "timing": "nextRestart",
            "presets": [{"name": "npm", "title": "npm", "hosts": ["registry.npmjs.org"]}],
            "presetUpdates": [{"name": "npm", "title": "npm", "addedHosts": ["npm.example"]}],
            "notices": ["A wildcard is not used as a network rule."]
        })
    }

    fn replies(method: MethodName, _params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            MethodName::EventsSubscribe => Ok(json!({"subscription": "sub"})),
            MethodName::ComputersList => Ok(json!({"linux": {"computers": [
                {"configuration": {"id": DEV, "name": "dev"}}
            ]}})),
            MethodName::NetworkPolicyGet | MethodName::NetworkPolicySet => {
                Ok(policy_state("allowlist"))
            }
            MethodName::GuestClientStatus
            | MethodName::GuestClientEnable
            | MethodName::GuestClientRevoke => Ok(
                json!({"computerId": DEV, "enabled": true, "network": "nextStart", "written": false}),
            ),
            _ => Ok(Value::Null),
        }
    }

    #[test]
    fn the_contract_fixtures_are_what_the_typed_results_read() {
        let fixtures: Value = serde_json::from_str(include_str!(
            "../../../src/test/contracts/network-policy.json"
        ))
        .unwrap();
        fn read<T: serde::de::DeserializeOwned>(fixtures: &Value, key: &str) -> T {
            serde_json::from_value(fixtures[key].clone()).unwrap()
        }
        let allowlist: network::NetworkPolicyState = read(&fixtures, "allowlist");
        assert_eq!(allowlist.policy.mode, network::NetworkPolicyMode::Allowlist);
        assert_eq!(allowlist.timing, network::NetworkPolicyTiming::NextRestart);
        assert_eq!(allowlist.preset_updates[0].added_hosts.len(), 2);
        assert_eq!(allowlist.notices.len(), 1);
        let open: network::NetworkPolicyState = read(&fixtures, "open");
        assert_eq!(open.policy.mode, network::NetworkPolicyMode::Open);
        assert_eq!(open.timing, network::NetworkPolicyTiming::Applied);
        let closed: network::NetworkPolicyState = read(&fixtures, "closed");
        assert_eq!(closed.policy.mode, network::NetworkPolicyMode::Closed);
        let on: GuestClientStatus = read(&fixtures, "toolsOn");
        assert!(on.enabled);
        assert_eq!(on.network, Some(network::NetworkPolicyTiming::NextStart));
        let off: GuestClientStatus = read(&fixtures, "toolsOff");
        assert!(!off.enabled);
    }

    #[tokio::test]
    async fn the_policy_is_read_and_saved_by_computer_id_with_the_one_the_person_saw() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let read = read_policy(&engine, "dev").await.unwrap();
        assert_eq!(read["policy"]["mode"], "allowlist");
        let seen = json!({"mode": "allowlist", "hosts": ["example.com"], "presets": ["npm"]});
        let wanted = json!({"mode": "closed", "hosts": [], "presets": []});
        save_policy(&engine, "dev", wanted.clone(), Some(seen.clone()))
            .await
            .unwrap();
        save_policy(&engine, "dev", wanted.clone(), None)
            .await
            .unwrap();
        assert_eq!(
            fake.calls(MethodName::NetworkPolicyGet),
            vec![json!({"computerId": DEV})]
        );
        assert_eq!(
            fake.calls(MethodName::NetworkPolicySet),
            vec![
                json!({"computerId": DEV, "policy": wanted, "expected": seen}),
                json!({"computerId": DEV, "policy": wanted}),
            ]
        );
    }

    #[tokio::test]
    async fn a_policy_the_engine_would_not_read_is_refused_before_the_call() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let error = save_policy(&engine, "dev", json!({"mode": "everything"}), None)
            .await
            .unwrap_err();
        assert!(error.message.starts_with("Invalid network policy"));
        assert!(fake.calls(MethodName::NetworkPolicySet).is_empty());
    }

    #[tokio::test]
    async fn the_engines_refusal_of_a_stale_save_reaches_the_window_in_its_words() {
        let fake = FakeEngine::start(|method, params| {
            match method {
            MethodName::NetworkPolicySet => Err((
                "changed",
                "The network policy of this computer changed while this was being decided. Read it again and retry.".to_owned(),
            )),
            other => replies(other, params),
        }
        });
        let (engine, _) = connected_client(&fake).await;
        let error = save_policy(
            &engine,
            "dev",
            json!({"mode": "open"}),
            Some(json!({"mode": "closed"})),
        )
        .await
        .unwrap_err();
        assert!(error.message.ends_with("Read it again and retry."));
    }

    #[tokio::test]
    async fn the_tools_are_read_turned_on_and_turned_off_by_computer_id() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(read_tools(&engine, "dev").await.unwrap()["enabled"], true);
        enable_tools(&engine, "dev").await.unwrap();
        revoke_tools(&engine, "dev").await.unwrap();
        for method in [
            MethodName::GuestClientStatus,
            MethodName::GuestClientEnable,
            MethodName::GuestClientRevoke,
        ] {
            assert_eq!(fake.calls(method), vec![json!({"computerId": DEV})]);
        }
    }

    #[tokio::test]
    async fn a_computer_that_does_not_exist_is_not_found() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(
            read_policy(&engine, "ghost").await.unwrap_err().code,
            "not_found"
        );
        assert_eq!(
            enable_tools(&engine, "ghost").await.unwrap_err().code,
            "not_found"
        );
        assert!(fake.calls(MethodName::NetworkPolicyGet).is_empty());
    }
}
