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
    assert!(store().verify(&token, COMPUTER).is_none());
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

#[tokio::test]
async fn turning_the_tools_off_owes_the_removal_of_the_registration_until_it_is_done() {
    let _held = TOKENS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let app_data = request_context().silod_context().paths().app_data.clone();
    store().enable(COMPUTER, 45125).unwrap();
    call(
        MethodName::GuestClientRevoke,
        json!({"computerId": COMPUTER}),
    )
    .await
    .unwrap();
    // The computer in the test engine is not running, so the removal waits for its next start.
    assert!(silo_engine::guest_components::owed_removals(&app_data).contains(&COMPUTER.to_owned()));
    // A computer that never had the tools owes nothing.
    call(
        MethodName::GuestClientRevoke,
        json!({"computerId": "never-had-the-tools"}),
    )
    .await
    .unwrap();
    assert!(!silo_engine::guest_components::owed_removals(&app_data)
        .contains(&"never-had-the-tools".to_owned()));
    silo_engine::guest_components::forget(&app_data.join("guest-client"), COMPUTER);
}

#[tokio::test]
async fn the_status_lists_what_silo_keeps_in_the_computer_and_names_a_program_it_cannot_provide() {
    let _held = TOKENS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    store().enable(COMPUTER, 45126).unwrap();
    let status = call(
        MethodName::GuestClientStatus,
        json!({"computerId": COMPUTER}),
    )
    .await
    .unwrap();
    let components = status["components"].as_array().unwrap();
    // The test engine ships no program, so the status says the build cannot give one.
    let silo = components.iter().find(|c| c["id"] == "silo").unwrap();
    assert_eq!(silo["state"], "unavailable");
    assert!(silo["detail"].as_str().unwrap().contains("Linux"));
    store().revoke(&[COMPUTER]).unwrap();
}

#[tokio::test]
async fn a_program_to_send_is_read_before_anything_is_opened() {
    let _held = TOKENS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let error = call(
        MethodName::GuestClientEnable,
        json!({"computerId": COMPUTER, "binaryPath": "/no/such/silo"}),
    )
    .await
    .unwrap_err();
    // Either the computer is not running or the file is missing; in both cases nothing is opened.
    assert!(
        error.message.contains("could not be read")
            || error.message.contains("Start the computer first")
            || error.message.contains("not found"),
        "{}",
        error.message
    );
    assert!(store().status(COMPUTER).unwrap().is_none());
}
