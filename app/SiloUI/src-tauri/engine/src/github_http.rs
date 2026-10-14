//! Token-management transport. Never replays a request inside an HTTP call.
//! The policy worker retries only the latest intent after these gates allow it.
use reqwest::{
    blocking::{Client, Response},
    header::HeaderMap,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    io::Read,
    sync::{Mutex, MutexGuard, OnceLock, PoisonError},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MAX_RETRIES: u32 = 5;
const MAX_RESPONSE: u64 = 8 * 1024 * 1024;
/// GitHub explicitly refused the stored refresh token.
const AUTHORIZATION_REJECTED: &str = "GitHub rejected the authorization.";
/// Whether GitHub rejected the refresh credential itself. App configuration and
/// transport failures do not prove that the stored credential cannot renew.
pub(crate) fn authorization_rejected(error: &str) -> bool {
    error.starts_with(AUTHORIZATION_REJECTED)
}
static CLIENT: OnceLock<Client> = OnceLock::new();
static GATES: OnceLock<Mutex<Gates>> = OnceLock::new();
/// GitHub limits each credential separately, so a limit reached with one credential
/// (such as the OAuth account) never delays another (such as a personal token).
#[derive(Default)]
struct Gates {
    requests: HashMap<String, Failure>,
    /// Server-imposed waiting deadline per rate class (see `rate_class`).
    rate_until: HashMap<String, u64>,
}
struct Failure {
    attempts: u32,
    until: Option<u64>,
    message: String,
    class: String,
    computer: Option<String>,
    safe: bool,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
// Retry gates are plain data that stays valid after a panic elsewhere (K-24).
fn gates() -> MutexGuard<'static, Gates> {
    GATES
        .get_or_init(|| Mutex::new(Gates::default()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}
fn client() -> Result<&'static Client, String> {
    if let Some(client) = CLIENT.get() {
        return Ok(client);
    }
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Silo")
        .build()
        .map_err(|_| "Cannot initialize GitHub connection.")?;
    let _ = CLIENT.set(client);
    CLIENT
        .get()
        .ok_or_else(|| "Cannot initialize GitHub connection.".into())
}
/// The credential a request is limited under. Tokens are identified only by a
/// truncated hash, which may be persisted; credentials never are.
fn rate_class(authentication: &Authentication) -> String {
    match authentication {
        Authentication::None => "oauth".into(),
        Authentication::Bearer(token) => {
            let mut hash = Sha256::new();
            hash.update(b"silo-github-rate-class\0");
            hash.update(token.as_bytes());
            format!("token:{}", &format!("{:x}", hash.finalize())[..16])
        }
        Authentication::App { client_id, .. } => format!("app:{client_id}"),
    }
}
fn key(route: &str, body: &[u8], computer: Option<&str>) -> String {
    // Only hashes identify failed requests; credentials never appear in diagnostics.
    let mut hash = Sha256::new();
    if let Some(computer) = computer {
        hash.update(b"silo-github-computer\0");
        hash.update(computer.len().to_le_bytes());
        hash.update(computer);
    }
    hash.update(route);
    hash.update(body);
    format!("{:x}", hash.finalize())
}
fn waiting(until: u64, at: u64) -> String {
    let seconds = until.saturating_sub(at).max(1);
    let unit = if seconds == 1 { "second" } else { "seconds" };
    format!("GitHub access update is waiting. Retrying in {seconds} {unit}.")
}
impl Gates {
    fn restore_floor(&mut self, class: &str, until: u64) {
        let floor = self.rate_until.entry(class.into()).or_default();
        *floor = (*floor).max(until);
    }
    fn floor(&self, class: &str) -> u64 {
        self.rate_until.get(class).copied().unwrap_or(0)
    }
    fn next_retry(&self, at: u64) -> u64 {
        // The worker's persisted deadline already wakes due requests. An old
        // superseded key must not keep scheduling successful work forever.
        self.requests
            .values()
            .filter_map(|failure| {
                failure
                    .until
                    .map(|until| until.max(self.floor(&failure.class)))
            })
            .chain(self.rate_until.values().copied())
            .filter(|until| *until > at)
            .min()
            .unwrap_or(0)
    }
    fn check(&self, key: &str, class: &str, at: u64) -> Result<(), String> {
        let floor = self.floor(class);
        if floor > at {
            return Err(waiting(floor, at));
        }
        if let Some(failure) = self.requests.get(key) {
            match failure.until {
                Some(until) if until > at => return Err(waiting(until, at)),
                None => return Err(failure.message.clone()),
                _ => {}
            }
        }
        Ok(())
    }
    fn fail(
        &mut self,
        key: String,
        class: &str,
        at: u64,
        retryable: bool,
        floor: u64,
        rate: bool,
        jitter: u64,
        message: &str,
        persistent: bool,
    ) -> String {
        let attempts = self
            .requests
            .get(&key)
            .map_or(1, |f| f.attempts.saturating_add(1));
        let base = if rate { 60u64 } else { 2 };
        let delay = base
            .saturating_mul(1u64 << attempts.saturating_sub(1).min(10))
            .min(900);
        let until = at
            .saturating_add(delay)
            .max(floor)
            .saturating_add(jitter % 4);
        if rate || floor > at {
            self.restore_floor(class, until);
        }
        // Safe reads (such as token validation) keep retrying with capped backoff;
        // a network outage must never leave them permanently stopped.
        let retry = retryable && (persistent || attempts <= MAX_RETRIES);
        let message = if retry {
            waiting(until, at)
        } else {
            format!("{message} Automatic retries stopped. Retry explicitly after checking GitHub access.")
        };
        self.requests.insert(
            key,
            Failure {
                attempts,
                until: retry.then_some(until),
                message: message.clone(),
                class: class.into(),
                computer: None,
                safe: persistent,
            },
        );
        message
    }
}
/// Restore only server-imposed deadlines per rate class, never request credentials or
/// per-request hashes. Relaunch must not bypass GitHub's requested waiting time.
pub(crate) fn restore_retry_floors(floors: &BTreeMap<String, u64>) {
    let mut g = gates();
    for (class, until) in floors {
        g.restore_floor(class, *until);
    }
}
/// Deadlines still in the future, for persistence across relaunch.
pub(crate) fn retry_floors() -> BTreeMap<String, u64> {
    let at = now();
    gates()
        .rate_until
        .iter()
        .filter(|(_, until)| **until > at)
        .map(|(class, until)| (class.clone(), *until))
        .collect()
}
pub(crate) fn retry_at() -> u64 {
    gates().next_retry(now())
}
pub(crate) fn reset_retries() {
    // An explicit Retry cannot bypass GitHub's requested waiting period.
    gates().requests.clear();
}
pub(crate) fn reset_bearer_retries(token: &str) {
    let class = rate_class(&Authentication::Bearer(token.into()));
    gates().requests.retain(|_, failure| failure.class != class);
}
pub(crate) fn reset_computer_retries(computer: &str) {
    gates()
        .requests
        .retain(|_, failure| failure.computer.as_deref() != Some(computer));
}
pub(crate) fn reset_catalog_retries(token: &str) {
    let class = rate_class(&Authentication::Bearer(token.into()));
    gates()
        .requests
        .retain(|_, failure| failure.class != class || failure.computer.is_some() || !failure.safe);
}
fn preflight(key: &str, class: &str) -> Result<(), String> {
    gates().check(key, class, now())
}
#[derive(Clone, Copy)]
struct RequestGate<'a> {
    key: &'a str,
    class: &'a str,
    computer: Option<&'a str>,
}
fn failure(
    request: RequestGate<'_>,
    retryable: bool,
    floor: u64,
    rate: bool,
    message: &str,
    safe: bool,
) -> String {
    let jitter = u64::from(uuid::Uuid::new_v4().as_bytes()[0]);
    let mut g = gates();
    let message = g.fail(
        request.key.into(),
        request.class,
        now(),
        retryable,
        floor,
        rate,
        jitter,
        message,
        safe,
    );
    g.requests.get_mut(request.key).unwrap().computer = request.computer.map(str::to_owned);
    message
}
fn number(headers: &HeaderMap, name: &str) -> Option<u64> {
    headers.get(name)?.to_str().ok()?.parse().ok()
}
fn retry_after(headers: &HeaderMap, at: u64) -> u64 {
    let value = headers.get("retry-after").and_then(|v| v.to_str().ok());
    let after = value
        .and_then(|v| {
            v.parse::<u64>()
                .ok()
                .map(|s| at.saturating_add(s))
                .or_else(|| {
                    time::OffsetDateTime::parse(v, &time::format_description::well_known::Rfc2822)
                        .ok()
                        .and_then(|t| u64::try_from(t.unix_timestamp()).ok())
                })
        })
        .unwrap_or(0);
    let reset = if number(headers, "x-ratelimit-remaining") == Some(0) {
        number(headers, "x-ratelimit-reset").unwrap_or(0)
    } else {
        0
    };
    after.max(reset)
}
fn is_rate_limit(status: u16, headers: &HeaderMap, body: &Value) -> bool {
    if status == 429 {
        return true;
    }
    if status != 403 {
        return false;
    }
    let message = body["message"]
        .as_str()
        .unwrap_or_default()
        .to_ascii_lowercase();
    number(headers, "x-ratelimit-remaining") == Some(0)
        || retry_after(headers, now()) > 0
        || body["code"] == "rate_limited"
        || message.contains("secondary rate limit")
        || message.contains("api rate limit exceeded")
        || message.contains("abuse detection")
}
fn retryable_response(status: u16, headers: &HeaderMap, body: &Value, safe: bool) -> bool {
    is_rate_limit(status, headers, body) || ((status == 408 || status >= 500) && safe)
}
/// A connection failure means nothing was sent, so even a non-idempotent request
/// can be retried automatically; only a failure after sending has an unknown outcome.
fn transport_retryable(safe: bool, sent: bool) -> bool {
    safe || !sent
}
fn response(
    request: RequestGate<'_>,
    result: Result<Response, reqwest::Error>,
    safe: bool,
    revoke: bool,
) -> Result<Value, String> {
    let response = result.map_err(|error| {
        let sent = !error.is_connect();
        failure(
            request,
            transport_retryable(safe, sent),
            0,
            false,
            if sent {
                "Cannot reach GitHub. The request outcome is unknown."
            } else {
                "Cannot reach GitHub."
            },
            safe,
        )
    })?;
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let mut bytes = Vec::new();
    response
        .take(MAX_RESPONSE + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| {
            failure(
                request,
                safe || retryable_response(status, &headers, &Value::Null, safe),
                retry_after(&headers, now()),
                is_rate_limit(status, &headers, &Value::Null),
                "GitHub returned an incomplete response.",
                safe,
            )
        })?;
    if bytes.len() as u64 > MAX_RESPONSE {
        return Err(failure(
            request,
            false,
            0,
            false,
            "GitHub response exceeds the supported size.",
            safe,
        ));
    }
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if revoke && matches!(status, 204 | 404) {
        gates().requests.remove(request.key);
        return Ok(serde_json::json!({"revoked":true}));
    }
    if (200..300).contains(&status) {
        if body.get("error").is_some() {
            let message = if body["error"] == "bad_refresh_token" {
                format!("{AUTHORIZATION_REJECTED} Connect GitHub again.")
            } else {
                "GitHub rejected the token request.".into()
            };
            return Err(failure(request, false, 0, false, &message, safe));
        }
        if body.is_null() {
            return Err(failure(
                request,
                safe,
                0,
                false,
                "GitHub returned an invalid response.",
                safe,
            ));
        }
        gates().requests.remove(request.key);
        return Ok(body);
    }
    let floor = retry_after(&headers, now());
    let rate = is_rate_limit(status, &headers, &body);
    let retryable = retryable_response(status, &headers, &body, safe);
    let message = if rate {
        "GitHub is limiting requests."
    } else if status == 401 {
        "GitHub authorization expired or was revoked. Reconnect GitHub."
    } else if status == 403 {
        "GitHub refused this request. Check the App's repository permissions."
    } else {
        "GitHub access could not be updated."
    };
    Err(failure(request, retryable, floor, rate, message, safe))
}
/// Only fixed GitHub destinations are accepted. Tokens never follow redirects.
pub(crate) enum Authentication {
    None,
    Bearer(String),
    App {
        client_id: String,
        client_secret: String,
    },
}
pub(crate) struct Request {
    pub method: reqwest::Method,
    pub url: String,
    pub authentication: Authentication,
    pub body: Value,
    pub safe: bool,
    pub revoke: bool,
}
pub(crate) fn send(request: Request) -> Result<Value, String> {
    send_scoped(request, None)
}
pub(crate) fn send_for_computer(request: Request, computer: &str) -> Result<Value, String> {
    send_scoped(request, Some(computer))
}
fn send_scoped(request: Request, computer: Option<&str>) -> Result<Value, String> {
    let url = reqwest::Url::parse(&request.url).map_err(|_| "Invalid GitHub destination.")?;
    if url.scheme() != "https"
        || !matches!(url.host_str(), Some("api.github.com" | "github.com"))
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("Invalid GitHub destination.".into());
    }
    let mut bytes =
        serde_json::to_vec(&request.body).map_err(|_| "Cannot encode GitHub request.")?;
    let class = rate_class(&request.authentication);
    let mut builder = client()?
        .request(request.method.clone(), url)
        .header("Accept", "application/json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    match request.authentication {
        Authentication::None => {}
        Authentication::Bearer(token) => {
            bytes.extend_from_slice(token.as_bytes());
            builder = builder.bearer_auth(token);
        }
        Authentication::App {
            client_id,
            client_secret,
        } => {
            bytes.extend_from_slice(client_id.as_bytes());
            bytes.push(0);
            bytes.extend_from_slice(client_secret.as_bytes());
            builder = builder.basic_auth(client_id, Some(client_secret));
        }
    }
    let key = key(
        &format!("{} {}", request.method, request.url),
        &bytes,
        computer,
    );
    preflight(&key, &class)?;
    if !request.body.is_null() {
        builder = builder.json(&request.body);
    }
    response(
        RequestGate {
            key: &key,
            class: &class,
            computer,
        },
        builder.send(),
        request.safe,
        request.revoke,
    )
}
pub(crate) fn github(token: &str, path: &str) -> Result<Value, String> {
    if !path.starts_with('/') || path.starts_with("//") {
        return Err("Invalid GitHub API path.".into());
    }
    send(Request {
        method: reqwest::Method::GET,
        url: format!("https://api.github.com{path}"),
        authentication: Authentication::Bearer(token.into()),
        body: Value::Null,
        safe: true,
        revoke: false,
    })
}

#[cfg(test)]
pub(crate) fn assert_bearer_retry_isolated(retry: impl FnOnce(&str)) {
    let _test_state = crate::test_support::global_state();
    let token = uuid::Uuid::new_v4().to_string();
    let personal = rate_class(&Authentication::Bearer(token.clone()));
    let other = rate_class(&Authentication::Bearer(uuid::Uuid::new_v4().to_string()));
    let app = format!("app:{}", uuid::Uuid::new_v4());
    let validation_key = uuid::Uuid::new_v4().to_string();
    let other_key = uuid::Uuid::new_v4().to_string();
    let mint_key = uuid::Uuid::new_v4().to_string();
    {
        let mut g = gates();
        for (key, class) in [
            (&validation_key, &personal),
            (&other_key, &other),
            (&mint_key, &app),
        ] {
            g.fail(
                key.clone(),
                class,
                100,
                false,
                0,
                false,
                0,
                "failed",
                key == &validation_key,
            );
            assert!(g.check(key, class, u64::MAX).is_err());
        }
        g.restore_floor(&personal, 5000);
    }
    retry(&token);
    let mut g = gates();
    assert!(g.check(&validation_key, &personal, 5000).is_ok());
    assert!(g.check(&validation_key, &personal, 4999).is_err());
    assert!(g.check(&other_key, &other, u64::MAX).is_err());
    assert!(g.check(&mint_key, &app, u64::MAX).is_err());
    g.requests.remove(&other_key);
    g.requests.remove(&mint_key);
    g.rate_until.remove(&personal);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_retry_preserves_ambiguous_writes_and_unrelated_reads() {
        let _test_state = crate::test_support::global_state();
        let token = uuid::Uuid::new_v4().to_string();
        let class = rate_class(&Authentication::Bearer(token.clone()));
        let other_class = rate_class(&Authentication::Bearer(uuid::Uuid::new_v4().to_string()));
        let read = uuid::Uuid::new_v4().to_string();
        let write = uuid::Uuid::new_v4().to_string();
        let computer_read = uuid::Uuid::new_v4().to_string();
        let other_read = uuid::Uuid::new_v4().to_string();
        {
            let mut g = gates();
            for (key, owner, safe) in [
                (&read, &class, true),
                (&write, &class, false),
                (&computer_read, &class, true),
                (&other_read, &other_class, true),
            ] {
                g.fail(key.clone(), owner, 100, false, 0, false, 0, "failed", safe);
            }
            g.requests.get_mut(&computer_read).unwrap().computer = Some("computer".into());
            g.restore_floor(&class, 5000);
        }
        reset_catalog_retries(&token);
        let mut g = gates();
        assert!(g.check(&read, &class, 5000).is_ok());
        assert!(g.check(&read, &class, 4999).is_err());
        assert!(g.check(&write, &class, u64::MAX).is_err());
        assert!(g.check(&computer_read, &class, u64::MAX).is_err());
        assert!(g.check(&other_read, &other_class, u64::MAX).is_err());
        for key in [&write, &computer_read, &other_read] {
            g.requests.remove(key);
        }
        g.rate_until.remove(&class);
    }
    #[test]
    fn computer_retry_preserves_other_computers_and_account_operations() {
        let _test_state = crate::test_support::global_state();
        let target = uuid::Uuid::new_v4().to_string();
        let other = uuid::Uuid::new_v4().to_string();
        let class = format!("app:{}", uuid::Uuid::new_v4());
        let target_key = key("POST /mint", b"same-body", Some(&target));
        let other_key = key("POST /mint", b"same-body", Some(&other));
        let account_key = key("POST /mint", b"same-body", None);
        assert_ne!(target_key, other_key);
        assert_ne!(target_key, account_key);
        for (key, computer) in [
            (&target_key, Some(target.as_str())),
            (&other_key, Some(other.as_str())),
            (&account_key, None),
        ] {
            wire_reply_for(
                RequestGate {
                    key,
                    class: &class,
                    computer,
                },
                "HTTP/1.1 502 Bad Gateway\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}"
                    .into(),
                false,
                false,
            )
            .unwrap_err();
        }
        gates().restore_floor(&class, 5000);
        reset_computer_retries(&target);
        let mut g = gates();
        assert!(g.check(&target_key, &class, 5000).is_ok());
        assert!(g.check(&target_key, &class, 4999).is_err());
        assert!(g.check(&other_key, &class, u64::MAX).is_err());
        assert!(g.check(&account_key, &class, u64::MAX).is_err());
        g.requests.remove(&other_key);
        g.requests.remove(&account_key);
        g.rate_until.remove(&class);
    }
    #[test]
    fn repository_refresh_preserves_ambiguous_writes_and_server_floors() {
        let _test_state = crate::test_support::global_state();
        let token = uuid::Uuid::new_v4().to_string();
        let class = rate_class(&Authentication::Bearer(token.clone()));
        let read_key = uuid::Uuid::new_v4().to_string();
        let mint_key = uuid::Uuid::new_v4().to_string();
        let refresh_key = uuid::Uuid::new_v4().to_string();
        {
            let mut g = gates();
            for (key, safe) in [(&read_key, true), (&mint_key, false), (&refresh_key, false)] {
                g.fail(key.clone(), &class, 100, false, 0, false, 0, "failed", safe);
                assert!(g.check(key, &class, u64::MAX).is_err());
            }
            g.restore_floor(&class, 5000);
        }
        reset_catalog_retries(&token);
        let mut g = gates();
        assert!(g.check(&read_key, &class, 5000).is_ok());
        assert!(g.check(&read_key, &class, 4999).is_err());
        assert!(
            g.check(&mint_key, &class, u64::MAX).is_err(),
            "Repository Refresh reopened an ambiguous mint"
        );
        assert!(
            g.check(&refresh_key, &class, u64::MAX).is_err(),
            "Repository Refresh replayed a rotating refresh token"
        );
        g.requests.remove(&mint_key);
        g.requests.remove(&refresh_key);
        g.rate_until.remove(&class);
    }
    #[test]
    fn personal_token_retry_preserves_unrelated_failures_and_server_floors() {
        assert_bearer_retry_isolated(reset_bearer_retries);
    }
    #[test]
    fn retry_gate_pluralizes_the_remaining_seconds() {
        let mut gates = Gates::default();
        gates.restore_floor("fixture", 102);
        assert_eq!(
            gates.check("request", "fixture", 100).unwrap_err(),
            "GitHub access update is waiting. Retrying in 2 seconds."
        );
        assert_eq!(
            gates.check("request", "fixture", 101).unwrap_err(),
            "GitHub access update is waiting. Retrying in 1 second."
        );
        assert!(gates.check("request", "fixture", 102).is_ok());
    }
    fn wire_response(status: u16, body: &str, revoke: bool) -> Result<Value, String> {
        let reply = format!(
            "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        wire_reply(
            &uuid::Uuid::new_v4().to_string(),
            "wire-test",
            reply,
            false,
            revoke,
        )
    }
    fn wire_reply(
        key: &str,
        class: &str,
        reply: String,
        safe: bool,
        revoke: bool,
    ) -> Result<Value, String> {
        wire_reply_for(
            RequestGate {
                key,
                class,
                computer: None,
            },
            reply,
            safe,
            revoke,
        )
    }
    fn wire_reply_for(
        request: RequestGate<'_>,
        reply: String,
        safe: bool,
        revoke: bool,
    ) -> Result<Value, String> {
        use std::io::Write;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            assert!(stream.read(&mut request).unwrap() > 0);
            stream.write_all(reply.as_bytes()).unwrap();
        });
        let result = response(
            request,
            Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap()
                .get(format!("http://{address}"))
                .send(),
            safe,
            revoke,
        );
        server.join().unwrap();
        result
    }
    #[test]
    fn request_timeouts_retry_safe_reads_without_replaying_unsafe_writes() {
        let _test_state = crate::test_support::global_state();
        for safe in [true, false] {
            let key = uuid::Uuid::new_v4().to_string();
            let class = uuid::Uuid::new_v4().to_string();
            let error = wire_reply(
                &key,
                &class,
                "HTTP/1.1 408 Request Timeout\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}"
                    .into(),
                safe,
                false,
            )
            .unwrap_err();
            let mut g = gates();
            if safe {
                let until = g.requests[&key]
                    .until
                    .expect("safe read stopped after HTTP 408");
                assert!(error.contains("Retrying"));
                assert!(g.check(&key, &class, until - 1).is_err());
                assert!(g.check(&key, &class, until).is_ok());
            } else {
                assert!(g.requests[&key].until.is_none());
                assert!(g.check(&key, &class, u64::MAX).is_err());
            }
            g.requests.remove(&key);
        }
        assert!(!retryable_response(
            401,
            &HeaderMap::new(),
            &Value::Null,
            true
        ));
        assert!(!retryable_response(
            404,
            &HeaderMap::new(),
            &Value::Null,
            true
        ));
    }
    #[test]
    fn interrupted_successful_body_retries_safe_reads_but_not_ambiguous_writes() {
        let _test_state = crate::test_support::global_state();
        for safe in [true, false] {
            let key = uuid::Uuid::new_v4().to_string();
            let class = uuid::Uuid::new_v4().to_string();
            let error = wire_reply(
                &key,
                &class,
                "HTTP/1.1 200 OK\r\nContent-Length: 20\r\nConnection: close\r\n\r\n{".into(),
                safe,
                false,
            )
            .unwrap_err();
            let until = gates().requests[&key].until;
            if safe {
                let until = until.expect("interrupted safe read stopped retrying");
                assert!(error.contains("Retrying"));
                assert!(gates().check(&key, &class, until - 1).is_err());
                assert!(gates().check(&key, &class, until).is_ok());
                assert_eq!(
                    wire_reply(
                        &key,
                        &class,
                        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}"
                            .into(),
                        safe,
                        false,
                    )
                    .unwrap(),
                    serde_json::json!({})
                );
                assert!(!gates().requests.contains_key(&key));
            } else {
                assert!(until.is_none());
                assert!(error.contains("Automatic retries stopped"));
                assert!(gates().check(&key, &class, u64::MAX).is_err());
                gates().requests.remove(&key);
            }
        }
    }
    #[test]
    fn service_unavailable_retry_after_survives_explicit_retry_and_relaunch() {
        let _test_state = crate::test_support::global_state();
        let key = uuid::Uuid::new_v4().to_string();
        let class = uuid::Uuid::new_v4().to_string();
        let at = now();
        let error = wire_reply(
            &key,
            &class,
            "HTTP/1.1 503 Service Unavailable\r\nRetry-After: 600\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".into(),
            true,
            false,
        )
        .unwrap_err();
        assert!(error.contains("Retrying"));
        let until = gates().requests[&key].until.unwrap();
        assert!(until >= at + 600);
        reset_retries();
        assert!(preflight(&key, &class).is_err());
        let floors = retry_floors();
        let mut restored = Gates::default();
        for (class, until) in floors {
            restored.restore_floor(&class, until);
        }
        assert!(restored.check(&key, &class, until - 1).is_err());
        assert!(restored.check(&key, &class, until).is_ok());
        assert!(restored.check("unrelated", "other-credential", at).is_ok());
        gates().rate_until.remove(&class);
    }
    #[test]
    fn real_http_oauth_errors_are_redacted_and_revocation_accepts_empty_responses() {
        let error = wire_response(
            200,
            r#"{"error":"bad_verification_code","error_description":"fixture-secret"}"#,
            false,
        )
        .unwrap_err();
        assert!(!error.contains("fixture-secret"));
        assert!(error.contains("rejected"));
        assert!(!authorization_rejected(&error));
        assert!(!authorization_rejected("Cannot reach GitHub."));
        assert_eq!(wire_response(204, "", true).unwrap()["revoked"], true);
        assert_eq!(wire_response(404, "", true).unwrap()["revoked"], true);
        assert!(wire_response(204, "", false).is_err());
        assert!(wire_response(302, "", false).is_err());
        assert!(wire_response(200, "not json", false).is_err());
    }
    #[test]
    fn oauth_configuration_errors_do_not_discard_a_refreshable_authorization() {
        for code in [
            "incorrect_client_credentials",
            "unsupported_grant_type",
            "unverified_user_email",
            "temporarily_unavailable",
        ] {
            let error = wire_response(
                200,
                &serde_json::json!({"error":code,"error_description":"fixture-secret"}).to_string(),
                false,
            )
            .unwrap_err();
            assert!(!error.contains("fixture-secret"));
            assert!(!authorization_rejected(&error), "{code}: {error}");
        }
        let error = wire_response(200, r#"{"error":"bad_refresh_token"}"#, false).unwrap_err();
        assert!(authorization_rejected(&error));
    }
    #[test]
    fn credential_destinations_are_fixed_before_network() {
        for url in [
            "http://api.github.com/user",
            "https://api.github.com.evil.test/user",
            "https://user@api.github.com/user",
            "https://github.com:444/user",
            "https://github.com/user#fragment",
        ] {
            let error = send(Request {
                method: reqwest::Method::POST,
                url: url.into(),
                authentication: Authentication::Bearer("fixture-secret".into()),
                body: Value::Null,
                safe: false,
                revoke: false,
            })
            .unwrap_err();
            assert_eq!(error, "Invalid GitHub destination.");
        }
    }
    #[test]
    fn expired_superseded_key_does_not_keep_scheduling_work() {
        let mut g = Gates::default();
        g.fail(
            "obsolete".into(),
            "c",
            100,
            true,
            0,
            false,
            0,
            "offline",
            false,
        );
        g.fail(
            "current".into(),
            "c",
            110,
            true,
            0,
            false,
            0,
            "offline",
            false,
        );
        assert_eq!(g.next_retry(101), 102);
        assert_eq!(g.next_retry(102), 112);
        assert_eq!(g.next_retry(112), 0);
        // Keep attempt counts and per-key refusal; only scheduling is filtered.
        assert!(g.check("obsolete", "c", 112).is_ok());
        assert_eq!(g.requests["obsolete"].attempts, 1);
        g.restore_floor("c", 200);
        assert_eq!(g.next_retry(112), 200);
        assert_eq!(g.next_retry(200), 0);
    }
    #[test]
    fn restored_rate_floor_survives_relaunch_and_cannot_be_shortened() {
        let mut g = Gates::default();
        g.restore_floor("c", 900);
        g.restore_floor("c", 500);
        assert!(g.check("new-session-request", "c", 899).is_err());
        assert!(g.check("new-session-request", "c", 900).is_ok());
        assert_eq!(g.floor("c"), 900);
        assert!(g.requests.is_empty());
    }
    #[test]
    fn direct_github_secondary_limits_are_distinct_from_permissions() {
        let mut headers = HeaderMap::new();
        let secondary = serde_json::json!({"message": "You have exceeded a secondary rate limit."});
        assert!(is_rate_limit(403, &headers, &secondary));
        assert!(!is_rate_limit(
            403,
            &headers,
            &serde_json::json!({"message": "Resource not accessible by integration"})
        ));
        headers.insert("x-ratelimit-remaining", "0".parse().unwrap());
        // An exhausted primary limit is still a limit when reset was omitted.
        assert!(is_rate_limit(403, &headers, &Value::Null));
        assert!(!is_rate_limit(401, &headers, &secondary));
    }
    #[test]
    fn upstream_body_cannot_authorize_replaying_ambiguous_writes() {
        let headers = HeaderMap::new();
        assert!(!retryable_response(502, &headers, &Value::Null, false));
        assert!(!retryable_response(
            502,
            &headers,
            &serde_json::json!({"retryable": false}),
            false
        ));
        assert!(!retryable_response(
            502,
            &headers,
            &serde_json::json!({"retryable": true}),
            false
        ));
        assert!(retryable_response(502, &headers, &Value::Null, true));
        assert!(!retryable_response(
            403,
            &headers,
            &serde_json::json!({"retryable": true}),
            false
        ));
        assert!(retryable_response(429, &headers, &Value::Null, false));
    }
    #[test]
    fn invalid_headers_do_not_classify_permissions_as_limits() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", "unknown".parse().unwrap());
        headers.insert("x-ratelimit-remaining", "invalid".parse().unwrap());
        headers.insert("x-ratelimit-reset", "900".parse().unwrap());
        assert_eq!(retry_after(&headers, 100), 0);
        assert!(!is_rate_limit(403, &headers, &Value::Null));
    }
    #[test]
    fn repeated_limits_back_off_and_stop_after_five_retries() {
        let mut g = Gates::default();
        let mut at = 100;
        let mut delays = Vec::new();
        for _ in 0..5 {
            g.fail("scope".into(), "c", at, true, 0, true, 0, "limit", false);
            let until = g.requests["scope"].until.unwrap();
            assert!(g.check("scope", "c", until - 1).is_err());
            assert!(g.check("scope", "c", until).is_ok());
            delays.push(until - at);
            at = until;
        }
        assert_eq!(delays, [60, 120, 240, 480, 900]);
        g.fail("scope".into(), "c", at, true, 0, true, 0, "limit", false);
        assert!(g.requests["scope"].until.is_none());
        assert!(g.check("scope", "c", u64::MAX).is_err());
    }
    #[test]
    fn server_wait_is_a_floor_and_jitter_only_extends_it() {
        let mut g = Gates::default();
        g.fail("a".into(), "c", 100, true, 5000, true, 3, "limit", false);
        assert_eq!(g.requests["a"].until, Some(5003));
        assert!(g.check("other", "c", 5002).is_err());
        // Explicit retry preserves the shared wait even after per-request reset.
        g.requests.clear();
        assert!(g.check("a", "c", 5002).is_err());
    }
    #[test]
    fn one_credentials_rate_limit_never_delays_another_credential() {
        let oauth = rate_class(&Authentication::Bearer("fixture-oauth-token".into()));
        let personal = rate_class(&Authentication::Bearer("fixture-personal-token".into()));
        let app = rate_class(&Authentication::App {
            client_id: "fixture".into(),
            client_secret: "fixture-secret".into(),
        });
        assert_ne!(oauth, personal);
        assert_eq!(
            oauth,
            rate_class(&Authentication::Bearer("fixture-oauth-token".into()))
        );
        // Classes may be persisted; they never contain a credential.
        assert!(!oauth.contains("fixture") && !personal.contains("fixture"));
        assert!(!app.contains("secret"));
        let mut g = Gates::default();
        // An OAuth secondary rate limit...
        g.fail(
            "catalog".into(),
            &oauth,
            100,
            true,
            5000,
            true,
            0,
            "limit",
            false,
        );
        assert!(g.check("other-oauth-request", &oauth, 4000).is_err());
        // ...does not fail the personal-token check or App token operations.
        assert!(g.check("personal-token-user", &personal, 100).is_ok());
        assert!(g.check("revoke", &app, 100).is_ok());
        // A personal-token failure is scheduled on its own, before the OAuth floor.
        g.fail(
            "personal-token-user".into(),
            &personal,
            100,
            true,
            0,
            false,
            0,
            "offline",
            true,
        );
        assert_eq!(g.next_retry(100), 102);
        assert_eq!(g.next_retry(102), 5000);
        // Persisted floors are restored per class.
        let mut restored = Gates::default();
        restored.restore_floor(&oauth, 5000);
        assert!(restored.check("catalog", &oauth, 4999).is_err());
        assert!(restored
            .check("personal-token-user", &personal, 4999)
            .is_ok());
    }
    #[test]
    fn ambiguous_mint_is_not_replayed_but_new_choice_can_proceed() {
        let mut g = Gates::default();
        g.fail("old".into(), "c", 100, false, 0, false, 0, "unknown", false);
        assert!(g.check("old", "c", u64::MAX).is_err());
        assert!(g.check("new", "c", 100).is_ok());
    }
    #[test]
    fn retry_headers_honor_both_deadlines_and_http_date() {
        let mut h = HeaderMap::new();
        h.insert("retry-after", "120".parse().unwrap());
        h.insert("x-ratelimit-remaining", "0".parse().unwrap());
        h.insert("x-ratelimit-reset", "900".parse().unwrap());
        assert_eq!(retry_after(&h, 100), 900);
        h.remove("x-ratelimit-remaining");
        assert_eq!(retry_after(&h, 100), 220);
        h.insert(
            "retry-after",
            "Wed, 21 Oct 2015 07:28:00 GMT".parse().unwrap(),
        );
        assert_eq!(retry_after(&h, 100), 1445412480);
    }
    #[test]
    fn safe_reads_keep_retrying_after_repeated_network_failures() {
        let mut g = Gates::default();
        let mut at = 100;
        for _ in 0..(MAX_RETRIES + 5) {
            g.fail("/user".into(), "c", at, true, 0, false, 0, "offline", true);
            at = g.requests["/user"]
                .until
                .expect("safe read stopped retrying");
        }
        assert!(g.check("/user", "c", at).is_ok());
        // Unsafe requests still stop to avoid repeating a side effect.
        let mut g = Gates::default();
        for _ in 0..=MAX_RETRIES {
            g.fail("post".into(), "c", 100, true, 0, false, 0, "offline", false);
        }
        assert_eq!(g.requests["post"].until, None);
    }
    #[test]
    fn unsent_requests_are_retryable_but_unknown_outcomes_are_not() {
        assert!(transport_retryable(false, false));
        assert!(!transport_retryable(false, true));
        assert!(transport_retryable(true, true));
    }
    #[test]
    fn transient_reads_back_off_without_delaying_unrelated_keys() {
        let mut g = Gates::default();
        g.fail("read".into(), "c", 100, true, 0, false, 0, "offline", false);
        assert!(g.check("read", "c", 101).is_err());
        assert!(g.check("read", "c", 102).is_ok());
        assert!(g.check("other", "c", 100).is_ok());
        g.fail("read".into(), "c", 102, true, 0, false, 0, "offline", false);
        assert_eq!(g.requests["read"].until, Some(106));
    }
}
