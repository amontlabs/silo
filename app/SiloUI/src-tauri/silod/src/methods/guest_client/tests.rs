use serde_json::{json, Value};
use silo_api::methods::MethodName;
use silo_engine::{context::EngineContext, guest_client::TokenStore};

use super::handle;
use crate::{
    dispatcher::DispatchError,
    methods::access::tests::{request_context, COMPUTER},
};

/// The tests share one token file.
static TOKENS: std::sync::Mutex<()> = std::sync::Mutex::new(());

async fn call(method: MethodName, params: Value) -> Result<Value, DispatchError> {
    handle(method, &params, &request_context())
        .await
        .expect("guestClient owns this method")
        .map(|output| output.value)
}

fn store() -> TokenStore {
    TokenStore::new(&request_context().silod_context().paths().app_data)
}

#[tokio::test]
async fn status_names_the_channel_and_never_the_token() {
    let _held = TOKENS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    store().enable(COMPUTER, 45123).unwrap();
    let token = store().mint(COMPUTER).unwrap().token;
    let status = call(
        MethodName::GuestClientStatus,
        json!({"computerId": COMPUTER}),
    )
    .await
    .unwrap();
    assert_eq!(status["enabled"], true);
    assert_eq!(status["host"], "host.microsandbox.internal");
    assert_eq!(status["port"], 45123);
    assert_eq!(status["endpointPath"], "/etc/silo/endpoint.json");
    assert_eq!(status["written"], false);
    assert!(!status.to_string().contains(&token));
    let app_data = request_context().silod_context().paths().app_data.clone();
    let stored = std::fs::read_to_string(app_data.join("guest-client/tokens.json")).unwrap();
    assert!(!stored.contains(&token));

    let revoked = call(
        MethodName::GuestClientRevoke,
        json!({"computerId": COMPUTER}),
    )
    .await
    .unwrap();
    assert_eq!(revoked["enabled"], false);
    assert!(revoked.get("port").is_none());
    let nonce = [1_u8; 32];
    let proof = silo_api::transport::to_hex(&silo_api::transport::channel_proof(
        &silo_api::transport::channel_key(&token),
        &nonce,
        45123,
    ));
    assert!(store().verify(COMPUTER, 45123, &nonce, &proof).is_none());
    assert_eq!(revoked["restartNeeded"], false);
}

#[tokio::test]
async fn enable_refuses_an_unknown_computer_and_opens_nothing() {
    let _held = TOKENS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let unknown = call(
        MethodName::GuestClientEnable,
        json!({"computerId": "no-such-computer"}),
    )
    .await
    .unwrap_err();
    assert!(unknown.message.contains("not found"), "{}", unknown.message);
    assert!(store().status("no-such-computer").unwrap().is_none());
}

#[tokio::test]
async fn a_promote_or_undo_turns_the_tools_off_through_one_helper() {
    let _held = TOKENS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    store().enable(COMPUTER, 45124).unwrap();
    super::revoke_for(&request_context(), &[COMPUTER]).unwrap();
    let status = call(
        MethodName::GuestClientStatus,
        json!({"computerId": COMPUTER}),
    )
    .await
    .unwrap();
    assert_eq!(status["enabled"], false);
}
