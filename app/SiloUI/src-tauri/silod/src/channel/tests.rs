use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use silo_engine::guest_client::{Channels as _, TokenStore};
use tokio::{io::AsyncWriteExt, net::TcpStream};

use silo_api::transport::{channel_key, channel_proof, channel_proof_matches, from_hex, to_hex};

use super::{
    check_handshake,
    watch::{
        adopt_earlier_records, due, prepare, rebind, release_held, retry_after,
        serve_after_restart, Running, Seen, MAX_CHECKS,
    },
    Channels, Refusal, Refusals, HANDSHAKE_TIMEOUT, MAX_CONNECTIONS, MAX_HANDSHAKE_BYTES, MAX_PENDING,
    REFUSAL_DELAY,
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

/// The ports each computer may be served on in these tests, with its token.
static PORTS: Mutex<Vec<(String, u16, &'static str)>> = Mutex::new(Vec::new());

fn verify(computer: &str, port: u16, nonce: &[u8], proof: &str) -> bool {
    let Some(proof) = from_hex::<32>(proof) else {
        return false;
    };
    let tokens = PORTS.lock().unwrap().clone();
    tokens.iter().any(|(id, at, token)| {
        id == computer
            && *at == port
            && channel_proof_matches(&channel_key(token), nonce, port, &proof)
    })
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

/// Serves `computer` on a fresh loopback port, where `token` proves it, and returns the port.
fn serve(channels: &Channels, computer: &str, token: &'static str) -> u16 {
    let (listener, port) = silo_engine::guest_client::bind(None).unwrap();
    PORTS
        .lock()
        .unwrap()
        .push((computer.to_owned(), port, token));
    channels.serve(computer, listener).unwrap();
    port
}

fn handshake(proof: Option<String>, extra: Value) -> Value {
    let mut params = json!({ "protocol": { "min": 8, "max": 8 } });
    if let Some(proof) = proof {
        params["proof"] = json!(proof);
    }
    if let (Some(params), Some(extra)) = (params.as_object_mut(), extra.as_object()) {
        params.extend(extra.clone());
    }
    json!({ "id": "handshake", "method": "engine.handshake", "params": params })
}

/// Connects and reads the challenge's nonce; `None` when the port closed instead.
async fn challenged(port: u16) -> (TcpStream, Option<[u8; 32]>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let challenge = tokio::time::timeout(
        Duration::from_secs(5),
        read_json_frame::<_, Value>(&mut stream),
    )
    .await
    .expect("the channel challenges or closes in time")
    .ok()
    .flatten();
    let nonce = challenge
        .as_ref()
        .and_then(|challenge| challenge["nonce"].as_str())
        .and_then(from_hex::<32>);
    (stream, nonce)
}

fn proof(token: &str, nonce: &[u8], port: u16) -> String {
    to_hex(&channel_proof(&channel_key(token), nonce, port))
}

/// Sends `first` after the challenge and returns the answer; `None` when the connection closed
/// with no answer.
async fn send_first(mut stream: TcpStream, first: &Value) -> (TcpStream, Option<Value>) {
    write_json_frame(&mut stream, first).await.unwrap();
    let answer = tokio::time::timeout(Duration::from_secs(5), read_json_frame(&mut stream))
        .await
        .expect("the channel answers or closes in time")
        .ok()
        .flatten();
    (stream, answer)
}

/// A full handshake on `port` proved with `token` for `proved_port`.
async fn connect(
    port: u16,
    token: &str,
    proved_port: u16,
    extra: Value,
) -> (TcpStream, Option<Value>) {
    let (stream, nonce) = challenged(port).await;
    let nonce = nonce.expect("a challenge");
    send_first(
        stream,
        &handshake(Some(proof(token, &nonce, proved_port)), extra),
    )
    .await
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

async fn ends(stream: &mut TcpStream) -> bool {
    let ended = tokio::time::timeout(
        Duration::from_secs(5),
        read_json_frame::<_, Value>(stream),
    )
    .await
    .expect("the connection ends in time");
    !matches!(ended, Ok(Some(_)))
}

#[tokio::test]
async fn a_valid_proof_is_served_and_every_request_is_its_computers() {
    let channels = channels();
    let port = serve(&channels, A, TOKEN_A);
    let (mut stream, answer) = connect(port, TOKEN_A, port, json!({})).await;
    let answer = answer.expect("a valid handshake is answered");
    assert_eq!(answer["result"]["protocol"], 8);
    assert_eq!(
        caller_of(&mut stream, "1").await,
        r#"Computer { id: "computer-a" }"#
    );
}

#[tokio::test]
async fn each_connection_gets_a_fresh_nonce_and_an_old_answer_is_refused() {
    let channels = channels();
    let port = serve(&channels, A, TOKEN_A);
    let (first, nonce) = challenged(port).await;
    let nonce = nonce.unwrap();
    drop(first);
    let (second, again) = challenged(port).await;
    assert_ne!(Some(nonce), again, "a nonce is never reused");
    // An answer captured for one challenge does not open another.
    let replayed = handshake(Some(proof(TOKEN_A, &nonce, port)), json!({}));
    assert!(send_first(second, &replayed).await.1.is_none());
}

#[tokio::test]
async fn a_proof_made_for_another_port_is_refused() {
    let channels = channels();
    let port = serve(&channels, A, TOKEN_A);
    let (_, answer) = connect(port, TOKEN_A, port.wrapping_add(1), json!({})).await;
    assert!(answer.is_none());
}

#[tokio::test]
async fn nothing_but_the_proof_and_the_port_chooses_the_computer() {
    let channels = channels();
    let port_a = serve(&channels, A, TOKEN_A);
    let port_b = serve(&channels, B, TOKEN_B);
    // Fields that name another computer, or the owner, or carry a token, change nothing.
    let claims = json!({
        "computerId": B,
        "client": { "name": B, "version": "0" },
        "sshKey": "ssh-ed25519 AAAA owner",
        "token": TOKEN_B,
    });
    let (mut stream, answer) = connect(port_a, TOKEN_A, port_a, claims).await;
    assert!(answer.is_some());
    assert_eq!(
        caller_of(&mut stream, "1").await,
        r#"Computer { id: "computer-a" }"#
    );
    // A's token on B's port, and B's on A's, are refused.
    assert!(connect(port_b, TOKEN_A, port_b, json!({})).await.1.is_none());
    assert!(connect(port_a, TOKEN_B, port_a, json!({})).await.1.is_none());
}

#[tokio::test]
async fn a_missing_or_wrong_proof_gets_no_answer_after_a_delay() {
    let channels = channels();
    let port = serve(&channels, A, TOKEN_A);
    for first in [
        handshake(None, json!({})),
        handshake(Some("00".repeat(32)), json!({})),
        handshake(Some(String::new()), json!({})),
        // The token itself is not a proof.
        handshake(None, json!({ "token": TOKEN_A })),
        json!({ "id": "1", "method": "computers.list", "params": {} }),
    ] {
        let (stream, nonce) = challenged(port).await;
        assert!(nonce.is_some());
        let begun = Instant::now();
        let (_, answer) = send_first(stream, &first).await;
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
    let port = serve(&channels, A, TOKEN_A);
    let (mut stream, _) = challenged(port).await;
    let length = u32::try_from(MAX_HANDSHAKE_BYTES + 1).unwrap();
    stream.write_all(&length.to_be_bytes()).await.unwrap();
    assert!(ends(&mut stream).await);
}

#[tokio::test]
async fn a_silent_connection_is_closed_after_the_handshake_time() {
    let channels = channels();
    let port = serve(&channels, A, TOKEN_A);
    let (mut stream, _) = challenged(port).await;
    let begun = Instant::now();
    assert!(ends(&mut stream).await);
    assert!(
        begun.elapsed() < HANDSHAKE_TIMEOUT + REFUSAL_DELAY + Duration::from_millis(500),
        "closed after {:?}",
        begun.elapsed()
    );
}

#[test]
fn the_handshake_check_needs_the_method_and_a_proof_for_this_computer_and_port() {
    PORTS.lock().unwrap().push((A.to_owned(), 41000, TOKEN_A));
    let nonce = [5_u8; 32];
    let check = |frame: &Value, computer: &str, port: u16| {
        check_handshake(frame, computer, port, &nonce, &verify)
    };
    let good = handshake(Some(proof(TOKEN_A, &nonce, 41000)), json!({}));
    assert_eq!(check(&good, A, 41000), Ok(()));
    assert_eq!(check(&good, B, 41000), Err(Refusal::BadHandshake));
    assert_eq!(check(&good, A, 41001), Err(Refusal::BadHandshake));
    assert_eq!(
        check(&handshake(None, json!({})), A, 41000),
        Err(Refusal::BadHandshake)
    );
    let mut renamed = good.clone();
    renamed["method"] = json!("engine.state");
    assert_eq!(check(&renamed, A, 41000), Err(Refusal::BadHandshake));
    let mut numeric = good;
    numeric["params"]["proof"] = json!(1);
    assert_eq!(check(&numeric, A, 41000), Err(Refusal::BadHandshake));
}

#[tokio::test]
async fn a_computer_holds_a_bounded_number_of_sessions() {
    let channels = channels();
    let port = serve(&channels, A, TOKEN_A);
    let mut held = Vec::new();
    for _ in 0..MAX_CONNECTIONS {
        let (stream, answer) = connect(port, TOKEN_A, port, json!({})).await;
        assert!(answer.is_some());
        held.push(stream);
    }
    let begun = Instant::now();
    let (_, answer) = connect(port, TOKEN_A, port, json!({})).await;
    assert!(answer.is_none());
    assert!(begun.elapsed() < REFUSAL_DELAY, "closed at once, not held");
    // Another computer's places are its own.
    let port_b = serve(&channels, B, TOKEN_B);
    assert!(connect(port_b, TOKEN_B, port_b, json!({})).await.1.is_some());
    drop(held);
}

#[tokio::test]
async fn connections_that_never_prove_anything_cannot_lock_sessions_out() {
    let channels = channels();
    let port = serve(&channels, A, TOKEN_A);
    // Fill every handshake place, and more, with connections that say nothing.
    let mut idle = Vec::new();
    for _ in 0..MAX_CONNECTIONS + MAX_PENDING {
        idle.push(TcpStream::connect(("127.0.0.1", port)).await.unwrap());
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    // While they hold their places, a real client is refused a place at once, but once they
    // time out it is served: they never take a session's place.
    let begun = Instant::now();
    loop {
        let (stream, nonce) = challenged(port).await;
        if let Some(nonce) = nonce {
            let answer = send_first(stream, &handshake(Some(proof(TOKEN_A, &nonce, port)), json!({}))).await.1;
            assert!(answer.is_some(), "a proved client is served");
            break;
        }
        assert!(
            begun.elapsed() < HANDSHAKE_TIMEOUT + Duration::from_secs(2),
            "the idle connections let go of their places"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    drop(idle);
    // Sessions keep their own places while connections wait in the handshake.
    let mut sessions = Vec::new();
    for _ in 0..MAX_CONNECTIONS - 1 {
        let (stream, answer) = connect(port, TOKEN_A, port, json!({})).await;
        assert!(answer.is_some());
        sessions.push(stream);
    }
    let mut waiting = Vec::new();
    for _ in 0..MAX_PENDING - 1 {
        waiting.push(challenged(port).await.0);
    }
    assert!(connect(port, TOKEN_A, port, json!({})).await.1.is_some());
}

#[tokio::test]
async fn turning_the_tools_off_ends_sessions_and_holds_the_port() {
    let channels = channels();
    let port = serve(&channels, A, TOKEN_A);
    let (mut stream, answer) = connect(port, TOKEN_A, port, json!({})).await;
    assert!(answer.is_some());
    assert_eq!(channels.serving(A), Some(port));
    channels.close(A);
    assert_eq!(channels.serving(A), None);
    assert!(ends(&mut stream).await, "the session ends");
    // The port stays bound: connections are accepted and closed with nothing sent.
    assert_eq!(channels.bound(port).as_deref(), Some(A));
    assert!(silo_engine::guest_client::bind_exact(port).is_err());
    let (mut held, nonce) = challenged(port).await;
    assert!(nonce.is_none(), "a held port sends nothing");
    assert!(ends(&mut held).await);
    // Served again, the same port answers; released, it is free.
    assert!(channels.serve_held(A, port));
    assert!(connect(port, TOKEN_A, port, json!({})).await.1.is_some());
    channels.close(A);
    channels.release(port);
    let mut free = false;
    for _ in 0..50 {
        if let Ok(listener) = silo_engine::guest_client::bind_exact(port) {
            drop(listener);
            free = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(free, "a released port is let go");
}

#[tokio::test]
async fn a_new_token_ends_the_computers_sessions_and_keeps_the_port_served() {
    let channels = channels();
    let port = serve(&channels, A, TOKEN_A);
    let port_b = serve(&channels, B, TOKEN_B);
    let (mut stream, answer) = connect(port, TOKEN_A, port, json!({})).await;
    assert!(answer.is_some());
    let (mut other, answer) = connect(port_b, TOKEN_B, port_b, json!({})).await;
    assert!(answer.is_some());
    channels.end_sessions(A);
    assert!(ends(&mut stream).await);
    assert_eq!(
        caller_of(&mut other, "1").await,
        r#"Computer { id: "computer-b" }"#,
        "another computer's sessions go on"
    );
    assert!(connect(port, TOKEN_A, port, json!({})).await.1.is_some());
}

#[test]
fn refusals_get_a_row_at_most_each_second_and_a_line_at_most_each_minute() {
    let mut refusals = Refusals::default();
    let start = Instant::now();
    assert_eq!(refusals.note(start), (true, Some(1)));
    let mut rows = 0;
    let mut lines = 0;
    for tenth in 1..=600 {
        let (row, line) = refusals.note(start + Duration::from_millis(100 * tenth));
        rows += u32::from(row);
        lines += u32::from(line.is_some());
    }
    assert!((59..=61).contains(&rows), "{rows} rows in a minute");
    assert_eq!(lines, 1, "one line a minute");
    let (_, line) = refusals.note(start + Duration::from_secs(121));
    assert_eq!(line, Some(1), "the line counts the refusals since the last one");
}

#[tokio::test]
async fn a_listener_not_on_the_loopback_is_never_served() {
    let channels = channels();
    let listener = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
    assert!(channels.serve(A, listener).is_err());
    assert_eq!(channels.serving(A), None);
}

/// Records what the watcher hands over, keeping the listeners bound.
#[derive(Default)]
struct Recorded(Mutex<Vec<(String, u16, bool, std::net::TcpListener)>>);

impl Recorded {
    fn entries(&self) -> Vec<(String, u16, bool)> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .map(|(id, port, served, _)| (id.clone(), *port, *served))
            .collect()
    }
}

impl silo_engine::guest_client::Channels for Recorded {
    fn serve(&self, computer_id: &str, listener: std::net::TcpListener) -> Result<(), String> {
        let port = listener.local_addr().unwrap().port();
        self.0
            .lock()
            .unwrap()
            .push((computer_id.to_owned(), port, true, listener));
        Ok(())
    }
    fn hold(&self, computer_id: &str, listener: std::net::TcpListener) -> Result<(), String> {
        let port = listener.local_addr().unwrap().port();
        self.0
            .lock()
            .unwrap()
            .push((computer_id.to_owned(), port, false, listener));
        Ok(())
    }
    fn serve_held(&self, computer_id: &str, port: u16) -> bool {
        let mut entries = self.0.lock().unwrap();
        match entries
            .iter_mut()
            .find(|(id, at, _, _)| id == computer_id && *at == port)
        {
            Some(entry) => {
                entry.2 = true;
                true
            }
            None => false,
        }
    }
    fn close(&self, _computer_id: &str) {}
    fn end_sessions(&self, _computer_id: &str) {}
    fn release(&self, port: u16) {
        self.0.lock().unwrap().retain(|(_, at, _, _)| *at != port);
    }
    fn serving(&self, computer_id: &str) -> Option<u16> {
        self.entries()
            .into_iter()
            .find(|(id, _, served)| id == computer_id && *served)
            .map(|(_, port, _)| port)
    }
    fn bound(&self, port: u16) -> Option<String> {
        self.entries()
            .into_iter()
            .find(|(_, at, _)| *at == port)
            .map(|(id, _, _)| id)
    }
}

fn accepts(store: &TokenStore, token: &str, computer: &str, port: u16) -> bool {
    let nonce = [1_u8; 32];
    store
        .verify(computer, port, &nonce, &proof(token, &nonce, port))
        .is_some()
}

#[test]
fn at_start_a_free_port_is_kept_and_a_taken_one_moves_held_until_a_restart() {
    let directory = tempfile::tempdir().unwrap();
    let store = TokenStore::new(directory.path());
    let (free, free_port) = silo_engine::guest_client::bind(None).unwrap();
    drop(free);
    let (taken, taken_port) = silo_engine::guest_client::bind(None).unwrap();
    store.enable(A, free_port).unwrap();
    store.enable(B, taken_port).unwrap();
    let recorded = Recorded::default();
    rebind(&store, &recorded);
    let served = recorded.entries();
    assert!(served.contains(&(A.to_owned(), free_port, true)));
    let (_, moved, served_b) = served.iter().find(|(id, _, _)| id == B).unwrap().clone();
    assert_ne!(moved, taken_port);
    assert!(!served_b, "not served while its network names the old port");
    let record = store.status(B).unwrap().unwrap();
    assert_eq!(record.port, Some(moved));
    assert!(record.restart_needed);
    assert!(store
        .locked()
        .held()
        .unwrap()
        .contains(&silo_engine::guest_client::HeldPort {
            computer_id: B.into(),
            port: taken_port
        }));
    // Its token stays dead and no proof opens it until it restarts with matching rules.
    let token = {
        store
            .update(B, |record| {
                record.restart_needed = false;
                Ok(())
            })
            .unwrap();
        let token = store.mint(B).unwrap().token;
        store
            .update(B, |record| {
                record.restart_needed = true;
                Ok(())
            })
            .unwrap();
        token
    };
    assert!(!accepts(&store, &token, B, moved));
    serve_after_restart(&store, &recorded, B).unwrap();
    assert_eq!(recorded.serving(B), Some(moved));
    assert!(!store.status(B).unwrap().unwrap().restart_needed);
    assert!(accepts(&store, &token, B, moved));
    drop(taken);
}

#[test]
fn at_start_every_token_is_dead_and_held_ports_are_bound_again() {
    let directory = tempfile::tempdir().unwrap();
    let store = TokenStore::new(directory.path());
    let policy = silo_engine::policy::store::PolicyStore::new(directory.path());
    let (free, port) = silo_engine::guest_client::bind(None).unwrap();
    drop(free);
    store.enable(A, port).unwrap();
    let token = store.mint(A).unwrap().token;
    assert!(accepts(&store, &token, A, port));
    let (old, old_port) = silo_engine::guest_client::bind(None).unwrap();
    drop(old);
    {
        let locked = store.locked();
        let mut held = locked.held().unwrap();
        silo_engine::guest_client::tokens::hold(&mut held, B, old_port);
        locked.save_held(&held).unwrap();
    }
    let recorded = Recorded::default();
    prepare(&store, &policy, &recorded);
    assert!(!accepts(&store, &token, A, port), "a token from before the start is dead");
    assert_eq!(recorded.serving(A), Some(port), "the tools stay on");
    assert_eq!(recorded.bound(old_port).as_deref(), Some(B));
    assert_eq!(recorded.serving(B), None);
    // Once B runs with rules that no longer name it, the port goes.
    let entry = silo_engine::guest_client::HeldPort {
        computer_id: B.into(),
        port: old_port,
    };
    release_held(&store, &recorded, &entry).unwrap();
    assert_eq!(recorded.bound(old_port), None);
    assert!(store.locked().held().unwrap().is_empty());
}

#[test]
fn checks_run_one_per_computer_and_a_bounded_number_at_once_with_a_growing_wait() {
    let running = Running::default();
    assert!(running.begin("a"));
    assert!(!running.begin("a"), "one check per computer");
    for index in 1..MAX_CHECKS {
        assert!(running.begin(&format!("c{index}")));
    }
    assert!(!running.begin("other"), "a bounded number at once");
    running.end("a");
    assert!(running.begin("other"));
    assert_eq!(retry_after(1), Duration::from_secs(5));
    assert_eq!(retry_after(2), Duration::from_secs(10));
    assert_eq!(retry_after(4), Duration::from_secs(40));
    assert_eq!(retry_after(30), Duration::from_secs(300));
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
