use serde_json::{json, Value};
use silo_api::methods::MethodName;
use silo_engine::context::EngineContext;

use super::handle;
use crate::{
    dispatcher::DispatchError,
    methods::access::tests::{request_context, COMPUTER},
};

pub(super) fn snapshot() -> crate::guest_bridge::Snapshot {
    crate::guest_bridge::Snapshot::from_values(
        &json!({"linux": {"computers": [{"configuration": {"id": COMPUTER}}]}}),
        &json!({"nodes": [{"id": COMPUTER, "name": "dev", "parent": null}]}),
    )
}

async fn call(method: MethodName, params: Value) -> Result<Value, DispatchError> {
    handle(method, &params, &request_context())
        .await
        .expect("guestClient owns this method")
        .map(|output| output.value)
}

fn policy() -> Value {
    json!({"fold": "ask", "forkSecrets": [], "agent": "openclaw"})
}

#[tokio::test]
async fn enable_returns_the_token_once_and_the_engine_keeps_only_its_hash() {
    let enabled = call(
        MethodName::GuestClientEnable,
        json!({"computerId": COMPUTER, "policy": policy()}),
    )
    .await
    .unwrap();
    let token = enabled["token"].as_str().unwrap().to_owned();
    assert!(token.starts_with("silo_gc_"));
    assert_eq!(enabled["tokenPath"], "/etc/silo/guest-client.json");
    assert_eq!(enabled["spoolDir"], "/.msb/silo-bridge");

    let status = call(MethodName::GuestClientStatus, json!({"computerId": COMPUTER}))
        .await
        .unwrap();
    assert_eq!(status["enabled"], true);
    assert_eq!(status["policy"]["fold"], "ask");
    assert!(!status.to_string().contains(&token));

    let app_data = request_context().silod_context().paths().app_data.clone();
    let stored = std::fs::read_to_string(app_data.join("guest-client/tokens.json")).unwrap();
    assert!(!stored.contains(&token));

    let again = call(
        MethodName::GuestClientEnable,
        json!({"computerId": COMPUTER, "policy": policy()}),
    )
    .await
    .unwrap();
    assert_ne!(again["token"], enabled["token"]);

    let revoked = call(MethodName::GuestClientRevoke, json!({"computerId": COMPUTER}))
        .await
        .unwrap();
    assert_eq!(revoked["enabled"], false);
}

#[tokio::test]
async fn enable_refuses_an_unknown_computer_and_an_unknown_fold_mode() {
    let unknown = call(
        MethodName::GuestClientEnable,
        json!({"computerId": "no-such-computer", "policy": policy()}),
    )
    .await
    .unwrap_err();
    assert!(unknown.message.contains("Linux computers only"));
    let bad = call(
        MethodName::GuestClientEnable,
        json!({"computerId": COMPUTER, "policy": {"fold": "always"}}),
    )
    .await
    .unwrap_err();
    assert!(bad.message.contains("ask or auto-code"));
}
