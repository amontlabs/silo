//! `HttpDownloader` against a scripted loopback HTTP server: full bodies, Range
//! resume, a dropped connection, redirects, servers that ignore Range, and
//! error statuses. Production only ever speaks HTTPS; the test-only
//! `plain_http` switch is the one difference.
use super::*;
use std::io::{BufRead, BufReader};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

#[derive(Clone, Copy)]
enum Step {
    /// 206 for a Range request, 200 otherwise.
    Serve,
    /// Always 200 with the whole body, ignoring any Range header.
    IgnoreRange,
    /// Headers for the whole (or remaining) body, `n` body bytes, then close.
    Cut(usize),
    /// 302 to `/final`.
    Redirect,
    /// A bare status line.
    Status(u16),
}

struct Server {
    url: String,
    /// The Range header (or empty) of every request, in order.
    ranges: Arc<Mutex<Vec<String>>>,
}

fn request_head(stream: &TcpStream) -> (String, String) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut path = String::new();
    let mut range = String::new();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    if let Some(p) = line.split_whitespace().nth(1) {
        path = p.to_owned();
    }
    loop {
        line.clear();
        if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("range:") {
            range = value.trim().to_owned();
        }
    }
    (path, range)
}

/// Serves `body` following `script`, one step per connection; the last step
/// repeats.
fn serve(body: Vec<u8>, script: Vec<Step>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let ranges = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&ranges);
    std::thread::spawn(move || {
        for (index, stream) in listener.incoming().enumerate() {
            let Ok(mut stream) = stream else { return };
            let (path, range) = request_head(&stream);
            seen.lock().unwrap().push(range.clone());
            let step = script[index.min(script.len() - 1)];
            let from = range
                .strip_prefix("bytes=")
                .and_then(|r| r.strip_suffix('-'))
                .and_then(|n| n.parse::<usize>().ok());
            let total = body.len();
            let head = |status: &str, start: usize| {
                let mut text =
                    format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\n", total - start);
                if status.starts_with("206") {
                    text.push_str(&format!(
                        "Content-Range: bytes {start}-{}/{total}\r\n",
                        total - 1
                    ));
                }
                text.push_str("Connection: close\r\n\r\n");
                text
            };
            let _ = match step {
                Step::Serve => match from {
                    Some(start) => stream
                        .write_all(head("206 Partial Content", start).as_bytes())
                        .and_then(|()| stream.write_all(&body[start..])),
                    None => stream
                        .write_all(head("200 OK", 0).as_bytes())
                        .and_then(|()| stream.write_all(&body)),
                },
                Step::IgnoreRange => stream
                    .write_all(head("200 OK", 0).as_bytes())
                    .and_then(|()| stream.write_all(&body)),
                Step::Cut(n) => {
                    let start = from.unwrap_or(0);
                    let status = if from.is_some() { "206 Partial Content" } else { "200 OK" };
                    stream
                        .write_all(head(status, start).as_bytes())
                        .and_then(|()| stream.write_all(&body[start..start + n]))
                }
                Step::Redirect if path == "/final" => stream
                    .write_all(head("200 OK", 0).as_bytes())
                    .and_then(|()| stream.write_all(&body)),
                Step::Redirect => stream.write_all(
                    format!(
                        "HTTP/1.1 302 Found\r\nLocation: http://{address}/final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    )
                    .as_bytes(),
                ),
                Step::Status(code) => stream.write_all(
                    format!("HTTP/1.1 {code} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                        .as_bytes(),
                ),
            };
        }
    });
    Server {
        url: format!("http://{address}/pkg.deb"),
        ranges,
    }
}

fn downloader() -> HttpDownloader {
    HttpDownloader {
        attempts: 5,
        backoff: Duration::from_millis(1),
        plain_http: true,
    }
}

fn payload() -> Vec<u8> {
    (0..300_000u32).map(|n| (n % 251) as u8).collect()
}

fn fetch(server: &Server, part: &Path, total: usize) -> (Result<(), Error>, Vec<u64>) {
    let mut seen = Vec::new();
    let result = downloader().fetch(&server.url, part, total as u64, &mut |n| seen.push(n));
    (result, seen)
}

#[test]
fn a_plain_download_writes_the_whole_body() {
    let body = payload();
    let server = serve(body.clone(), vec![Step::Serve]);
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("p.part");
    let (result, seen) = fetch(&server, &part, body.len());
    result.unwrap();
    assert_eq!(fs::read(&part).unwrap(), body);
    assert_eq!(*seen.last().unwrap(), body.len() as u64);
    assert_eq!(*server.ranges.lock().unwrap(), vec![String::new()]);
}

#[test]
fn a_dropped_connection_resumes_with_a_range_request() {
    let body = payload();
    let server = serve(body.clone(), vec![Step::Cut(100_000), Step::Serve]);
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("p.part");
    let (result, _) = fetch(&server, &part, body.len());
    result.unwrap();
    assert_eq!(fs::read(&part).unwrap(), body);
    assert_eq!(
        *server.ranges.lock().unwrap(),
        vec![String::new(), "bytes=100000-".to_owned()]
    );
}

#[test]
fn repeated_drops_keep_making_progress() {
    let body = payload();
    let server = serve(
        body.clone(),
        vec![
            Step::Cut(50_000),
            Step::Cut(50_000),
            Step::Cut(50_000),
            Step::Serve,
        ],
    );
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("p.part");
    let (result, _) = fetch(&server, &part, body.len());
    result.unwrap();
    assert_eq!(fs::read(&part).unwrap(), body);
    assert_eq!(
        *server.ranges.lock().unwrap(),
        vec![
            String::new(),
            "bytes=50000-".to_owned(),
            "bytes=100000-".to_owned(),
            "bytes=150000-".to_owned()
        ]
    );
}

#[test]
fn an_existing_prefix_is_resumed() {
    let body = payload();
    let server = serve(body.clone(), vec![Step::Serve]);
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("p.part");
    fs::write(&part, &body[..70_000]).unwrap();
    let (result, seen) = fetch(&server, &part, body.len());
    result.unwrap();
    assert_eq!(fs::read(&part).unwrap(), body);
    assert!(seen.iter().all(|n| *n >= 70_000));
    assert_eq!(
        *server.ranges.lock().unwrap(),
        vec!["bytes=70000-".to_owned()]
    );
}

#[test]
fn a_server_that_ignores_range_restarts_cleanly() {
    let body = payload();
    let server = serve(body.clone(), vec![Step::IgnoreRange]);
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("p.part");
    fs::write(&part, &body[..70_000]).unwrap();
    let (result, _) = fetch(&server, &part, body.len());
    result.unwrap();
    assert_eq!(fs::read(&part).unwrap(), body);
}

#[test]
fn redirects_are_followed() {
    let body = payload();
    let server = serve(body.clone(), vec![Step::Redirect]);
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("p.part");
    let (result, _) = fetch(&server, &part, body.len());
    result.unwrap();
    assert_eq!(fs::read(&part).unwrap(), body);
}

#[test]
fn a_redirect_to_plain_http_is_refused_in_production_mode() {
    let body = payload();
    let server = serve(body.clone(), vec![Step::Redirect]);
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("p.part");
    let mut strict = downloader();
    strict.plain_http = false;
    strict.attempts = 1;
    let result = strict.fetch(&server.url, &part, body.len() as u64, &mut |_| {});
    assert!(result.is_err());
    assert!(!part.exists());
}

#[test]
fn a_larger_body_than_pinned_is_discarded() {
    let body = payload();
    let server = serve(body.clone(), vec![Step::Serve]);
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("p.part");
    let mut one = downloader();
    one.attempts = 1;
    let error = one
        .fetch(&server.url, &part, body.len() as u64 - 10, &mut |_| {})
        .unwrap_err();
    assert!(error.retryable);
    assert!(!part.exists());
}

#[test]
fn a_missing_package_is_fatal_and_a_server_error_is_retried() {
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("p.part");
    let missing = serve(payload(), vec![Step::Status(404)]);
    let (result, _) = fetch(&missing, &part, 300_000);
    assert!(!result.unwrap_err().retryable);
    assert_eq!(missing.ranges.lock().unwrap().len(), 1);

    let flaky = serve(
        payload(),
        vec![Step::Status(503), Step::Status(503), Step::Serve],
    );
    let (result, _) = fetch(&flaky, &part, 300_000);
    result.unwrap();
    assert_eq!(fs::read(&part).unwrap(), payload());
}

#[test]
fn a_planted_link_at_the_part_name_is_removed_not_followed() {
    let body = payload();
    let server = serve(body.clone(), vec![Step::Serve]);
    let dir = tempfile::tempdir().unwrap();
    let victim = dir.path().join("victim");
    fs::write(&victim, b"keep").unwrap();
    let part = dir.path().join("p.part");
    std::os::unix::fs::symlink(&victim, &part).unwrap();
    let (result, _) = fetch(&server, &part, body.len());
    result.unwrap();
    assert_eq!(fs::read(&victim).unwrap(), b"keep");
    assert_eq!(fs::read(&part).unwrap(), body);
}

#[test]
fn an_unreachable_server_fails_with_a_retryable_error() {
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("p.part");
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut one = downloader();
    one.attempts = 2;
    let error = one
        .fetch(
            &format!("http://127.0.0.1:{port}/x"),
            &part,
            10,
            &mut |_| {},
        )
        .unwrap_err();
    assert!(error.retryable);
}
