use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use silo_engine::guest_client::{Channels as _, TokenStore};
use tokio::{io::AsyncWriteExt, net::TcpStream};

use super::{
    check_handshake,
    watch::{adopt_earlier_records, due, rebind, Seen},
    Channels, Refusal, MAX_CONNECTIONS, MAX_HANDSHAKE_BYTES, REFUSAL_DELAY,
};
use crate::{
    dispatcher::{
        DispatchError, Dispatcher, HandlerFuture, HandlerOutput, MethodRegistry, RequestContext,
        StreamFuture,
    },
    framing::{read_json_frame, write_json_frame},
};

/// Answers every method with the caller the session gave it.
struct EchoCaller;

impl MethodRegistry for EchoCaller {
    fn descriptors(&self) -> &[silo_api::methods::MethodDescriptor] {
        silo_api::methods::method_registry()
    }

    fn dispatch<'a>(
        &'a self,
        _method: silo_api::methods::MethodName,
        _params: Value,
        _context: Arc<crate::context::SilodContext>,
        request: RequestContext,
    ) -> HandlerFuture<'a> {
        Box::pin(async move {
            Ok(HandlerOutput {
                value: json!({ "caller": format!("{:?}", request.caller) }),
                event_barrier: None,
            })
        })
    }

    fn open_stream<'a>(
        &'a self,
        method: silo_api::methods::MethodName,
        _params: Value,
        _context: Arc<crate::context::SilodContext>,
        _request: RequestContext,
    ) -> StreamFuture<'a> {
        Box::pin(async move {
            Err(DispatchError::unsupported(crate::dispatcher::method_name(
                method,
            )))
        })
    }
}

const A: &str = "computer-a";
const B: &str = "computer-b";
const TOKEN_A: &str = "silo_gc_aaaa";
const TOKEN_B: &str = "silo_gc_bbbb";

fn verify(token: &str, computer: &str) -> bool {
    matches!((token, computer), (TOKEN_A, A) | (TOKEN_B, B))
}

fn channels() -> Arc<Channels> {
    Arc::new(Channels::new(
        Arc::new(Dispatcher::new(
            Arc::new(EchoCaller),
            crate::socket::tests::test_context(),
        )),
        tokio::runtime::Handle::current(),
        Arc::new(verify),
        None,
    ))
}

/// Serves `computer` on a fresh loopback port and returns the port.
fn serve(channels: &Channels, computer: &str) -> u16 {
    let (listener, port) = silo_engine::guest_client::bind(None).unwrap();
    channels.serve(computer, listener);
    port
}

fn handshake(token: Option<&str>, extra: Value) -> Value {
    let mut params = json!({ "protocol": { "min": 8, "max": 8 } });
    if let Some(token) = token {
        params["token"] = json!(token);
    }
    if let (Some(params), Some(extra)) = (params.as_object_mut(), extra.as_object()) {
        params.extend(extra.clone());
    }
    json!({ "id": "handshake", "method": "engine.handshake", "params": params })
}

/// Sends `first` and returns the answer; `None` when the connection closed with no answer.
async fn first_answer(port: u16, first: &Value) -> (TcpStream, Option<Value>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    write_json_frame(&mut stream, first).await.unwrap();
    let answer = tokio::time::timeout(Duration::from_secs(5), read_json_frame(&mut stream))
        .await
        .expect("the channel answers or closes in time")
        .ok()
        .flatten();
    (stream, answer)
}

async fn caller_of(stream: &mut TcpStream, id: &str) -> String {
    write_json_frame(
        stream,
        &json!({ "id": id, "method": "computers.list", "params": {} }),
    )
    .await
    .unwrap();
    let reply: Value = tokio::time::timeout(Duration::from_secs(5), read_json_frame(stream))
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    reply["result"]["caller"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn a_valid_token_is_served_and_every_request_is_its_computers() {
    let channels = channels();
    let port = serve(&channels, A);
    let (mut stream, answer) = first_answer(port, &handshake(Some(TOKEN_A), json!({}))).await;
    let answer = answer.expect("a valid handshake is answered");
    assert_eq!(answer["result"]["protocol"], 8);
    assert_eq!(
        caller_of(&mut stream, "1").await,
        r#"Computer { id: "computer-a" }"#
    );
}

#[tokio::test]
async fn nothing_but_the_token_and_the_port_chooses_the_computer() {
    let channels = channels();
    let port_a = serve(&channels, A);
    let port_b = serve(&channels, B);
    // Fields that name another computer, or the owner, change nothing.
    let claims = json!({
        "computerId": B,
        "client": { "name": B, "version": "0" },
        "sshKey": "ssh-ed25519 AAAA owner",
    });
    let (mut stream, answer) = first_answer(port_a, &handshake(Some(TOKEN_A), claims)).await;
    assert!(answer.is_some());
    assert_eq!(
        caller_of(&mut stream, "1").await,
        r#"Computer { id: "computer-a" }"#
    );
    // A's token on B's port, and B's on A's, are refused.
    assert!(first_answer(port_b, &handshake(Some(TOKEN_A), json!({})))
        .await
        .1
        .is_none());
    assert!(first_answer(port_a, &handshake(Some(TOKEN_B), json!({})))
        .await
        .1
        .is_none());
}

#[tokio::test]
async fn a_missing_or_wrong_token_gets_no_answer_after_a_delay() {
    let channels = channels();
    let port = serve(&channels, A);
    for first in [
        handshake(None, json!({})),
        handshake(Some("silo_gc_wrong"), json!({})),
        handshake(Some(""), json!({})),
        json!({ "id": "1", "method": "computers.list", "params": {} }),
        json!({ "id": "1", "method": "computers.list", "params": { "token": TOKEN_A } }),
    ] {
        let begun = Instant::now();
        let (_, answer) = first_answer(port, &first).await;
        assert!(answer.is_none(), "{first}");
        assert!(
            begun.elapsed() >= REFUSAL_DELAY - Duration::from_millis(50),
            "refused after {:?}",
            begun.elapsed()
        );
    }
}

#[tokio::test]
async fn an_oversized_handshake_is_refused_before_it_is_read() {
    let channels = channels();
    let port = serve(&channels, A);
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let length = u32::try_from(MAX_HANDSHAKE_BYTES + 1).unwrap();
    stream.write_all(&length.to_be_bytes()).await.unwrap();
    let answer = tokio::time::timeout(
        Duration::from_secs(5),
        read_json_frame::<_, Value>(&mut stream),
    )
    .await
    .unwrap();
    assert!(!matches!(answer, Ok(Some(_))));
}

#[test]
fn the_handshake_check_needs_the_method_and_this_computers_token() {
    let check = |frame: &Value, computer: &str| check_handshake(frame, computer, &verify);
    assert_eq!(check(&handshake(Some(TOKEN_A), json!({})), A), Ok(()));
    assert_eq!(
        check(&handshake(Some(TOKEN_A), json!({})), B),
        Err(Refusal::BadHandshake)
    );
    assert_eq!(
        check(&handshake(None, json!({})), A),
        Err(Refusal::BadHandshake)
    );
    let mut renamed = handshake(Some(TOKEN_A), json!({}));
    renamed["method"] = json!("engine.state");
    assert_eq!(check(&renamed, A), Err(Refusal::BadHandshake));
    let mut numeric = handshake(None, json!({}));
    numeric["params"]["token"] = json!(1);
    assert_eq!(check(&numeric, A), Err(Refusal::BadHandshake));
}

#[tokio::test]
async fn a_computer_holds_a_bounded_number_of_connections() {
    let channels = channels();
    let port = serve(&channels, A);
    let mut held = Vec::new();
    for _ in 0..MAX_CONNECTIONS {
        let (stream, answer) = first_answer(port, &handshake(Some(TOKEN_A), json!({}))).await;
        assert!(answer.is_some());
        held.push(stream);
    }
    let begun = Instant::now();
    let mut extra = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let closed = tokio::time::timeout(
        Duration::from_secs(5),
        read_json_frame::<_, Value>(&mut extra),
    )
    .await
    .unwrap();
    assert!(!matches!(closed, Ok(Some(_))));
    assert!(begun.elapsed() < REFUSAL_DELAY, "closed at once, not held");
    // Another computer's places are its own.
    let port_b = serve(&channels, B);
    assert!(first_answer(port_b, &handshake(Some(TOKEN_B), json!({})))
        .await
        .1
        .is_some());
    drop(held);
}

#[tokio::test]
async fn turning_the_tools_off_ends_open_sessions_and_the_port() {
    let channels = channels();
    let port = serve(&channels, A);
    let (mut stream, answer) = first_answer(port, &handshake(Some(TOKEN_A), json!({}))).await;
    assert!(answer.is_some());
    assert!(channels.serving(A));
    channels.close(A);
    assert!(!channels.serving(A));
    let ended = tokio::time::timeout(
        Duration::from_secs(5),
        read_json_frame::<_, Value>(&mut stream),
    )
    .await
    .expect("the session ends");
    assert!(!matches!(ended, Ok(Some(_))));
    let mut refused = false;
    for _ in 0..50 {
        if TcpStream::connect(("127.0.0.1", port)).await.is_err() {
            refused = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(refused, "the port is closed");
}

#[tokio::test]
async fn a_listener_not_on_the_loopback_is_never_served() {
    let channels = channels();
    let listener = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
    channels.serve(A, listener);
    assert!(!channels.serving(A));
}

/// Records what the watcher hands over.
#[derive(Default)]
struct Recorded(Mutex<Vec<(String, u16)>>);

impl silo_engine::guest_client::Channels for Recorded {
    fn serve(&self, computer_id: &str, listener: std::net::TcpListener) {
        let port = listener.local_addr().unwrap().port();
        self.0.lock().unwrap().push((computer_id.to_owned(), port));
    }
    fn close(&self, _computer_id: &str) {}
    fn serving(&self, _computer_id: &str) -> bool {
        false
    }
}

#[test]
fn at_start_a_free_port_is_kept_and_a_taken_one_moves_with_no_valid_token() {
    let directory = tempfile::tempdir().unwrap();
    let store = TokenStore::new(directory.path());
    let (free, free_port) = silo_engine::guest_client::bind(None).unwrap();
    drop(free);
    let (taken, taken_port) = silo_engine::guest_client::bind(None).unwrap();
    store.enable(A, free_port).unwrap();
    store.enable(B, taken_port).unwrap();
    let token_a = store.mint(A).unwrap().token;
    let token_b = store.mint(B).unwrap().token;
    let recorded = Recorded::default();
    rebind(&store, &recorded);
    let served = recorded.0.lock().unwrap().clone();
    assert!(served.contains(&(A.to_owned(), free_port)));
    let moved = served.iter().find(|(id, _)| id == B).unwrap().1;
    assert_ne!(moved, taken_port);
    assert_eq!(store.port_of(B), Some(moved));
    assert!(store.verify(&token_a, A).is_some());
    assert!(
        store.verify(&token_b, B).is_none(),
        "the token may have gone to the other program"
    );
    drop(taken);
}

#[test]
fn records_of_the_earlier_client_are_adopted_and_turned_off() {
    let directory = tempfile::tempdir().unwrap();
    let store = TokenStore::new(directory.path());
    let policy = silo_engine::policy::store::PolicyStore::new(directory.path());
    std::fs::create_dir_all(directory.path().join("guest-client")).unwrap();
    std::fs::write(
        directory.path().join("guest-client/tokens.json"),
        format!(
            r#"[{{"computerId":"{A}","hash":"ab","mintedAt":"x","policy":{{"forkSecrets":[]}},"forkIds":["{B}"],"jobs":[{{"id":"job-1","at":5}}]}}]"#
        ),
    )
    .unwrap();
    store.enable("computer-c", 40123).unwrap();
    adopt_earlier_records(&store, &policy);
    assert!(store
        .all()
        .unwrap()
        .iter()
        .all(|record| record.computer_id != A));
    assert_eq!(store.port_of("computer-c"), Some(40123));
    let document = policy.snapshot().unwrap();
    assert!(document.made_by(A).contains(B));
    assert_eq!(document.jobs_of(A).len(), 1);
}

#[test]
fn the_endpoint_file_is_looked_at_on_each_start_after_changes_and_now_and_then() {
    let now = Instant::now();
    let fresh = Seen::default();
    assert!(due(&fresh, true, 0, now), "first sight while running");
    assert!(!due(&fresh, false, 0, now), "never while stopped");
    let checked = Seen::for_test(true, 3, now, None);
    assert!(!due(&checked, true, 3, now + Duration::from_secs(1)));
    assert!(
        !due(&checked, true, 4, now + Duration::from_secs(1)),
        "a change waits a little"
    );
    assert!(due(&checked, true, 4, now + Duration::from_secs(31)));
    assert!(due(&checked, true, 3, now + Duration::from_secs(601)));
    let restarted = Seen::for_test(false, 3, now, None);
    assert!(
        due(&restarted, true, 3, now + Duration::from_secs(1)),
        "a start"
    );
    let failed = Seen::for_test(true, 3, now, Some(now + Duration::from_secs(5)));
    assert!(!due(&failed, true, 3, now + Duration::from_secs(1)));
    assert!(due(&failed, true, 3, now + Duration::from_secs(6)));
}
