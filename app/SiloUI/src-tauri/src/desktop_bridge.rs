//! Rust half of the bridge between Silo and the guest desktop page.
//!
//! The guest serves the page, so everything the page says is untrusted. Rust
//! drives the page with `Webview::eval` (payloads are always JSON-encoded) and
//! accepts an answer only on the proxy's reserved `/__silo/v1/<op>` route, and
//! only for a single-use nonce that Rust issued for that operation moments
//! earlier. See `desktop_viewer_bridge.js` for the page half.
// The audio, resize and image operations have no caller until their phases land.
#![allow(dead_code)]
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Mutex,
    },
    time::{Duration, Instant},
};

pub(crate) const ROUTE_PREFIX: &str = "/__silo/v1/";
/// How long a nonce stays valid beyond the wait the caller asked for.
const REPLY_GRACE: Duration = Duration::from_secs(5);
/// Longest wait a caller can request for the page's answer.
const MAX_WAIT: Duration = Duration::from_secs(10);
/// How long a send waits for the page to say the socket took its frames.
const SEND_WAIT: Duration = Duration::from_secs(3);
const NOT_CONNECTED: &str = "The desktop is not connected.";

/// Largest text Silo writes into a computer's clipboard.
pub(crate) const MAX_TEXT_BYTES: usize = 1024 * 1024;
/// Largest encoded image Silo writes into a computer's clipboard.
pub(crate) const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;
/// Selkies' own chunk size for clipboard transfers (`CLIPBOARD_CHUNK_SIZE`,
/// websockets_mode.py): 16 KiB rounded down to a multiple of three bytes so
/// every chunk is independently valid base64.
const CHUNK_BYTES: usize = 16 * 1024 / 3 * 3;

const MAX_SCREEN_EDGE: u32 = 4080;
const KEYSYM_CONTROL_L: u32 = 0xffe3;
const KEYSYM_C: u32 = b'c' as u32;
const KEYSYM_V: u32 = b'v' as u32;

/// Operations the page may answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Op {
    Clipboard,
    Capabilities,
    /// The page's acknowledgement that it sent (or could not send) frames.
    Sent,
    /// A stream benchmark report; development builds only.
    #[cfg(debug_assertions)]
    Diagnostics,
}
impl Op {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "clipboard" => Some(Self::Clipboard),
            "capabilities" => Some(Self::Capabilities),
            "sent" => Some(Self::Sent),
            #[cfg(debug_assertions)]
            "diagnostics" => Some(Self::Diagnostics),
            _ => None,
        }
    }
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Clipboard => "clipboard",
            Self::Capabilities => "capabilities",
            Self::Sent => "sent",
            #[cfg(debug_assertions)]
            Self::Diagnostics => "diagnostics",
        }
    }
    /// Largest request body Rust accepts for this operation.
    pub(crate) fn max_bytes(self) -> usize {
        match self {
            Self::Clipboard => 24 * 1024 * 1024,
            Self::Capabilities => 4 * 1024,
            Self::Sent => 0,
            #[cfg(debug_assertions)]
            Self::Diagnostics => 4 * 1024 * 1024,
        }
    }
    /// Acknowledgements for several sends and answers to several capability
    /// questions (the sound probe and the clipboard policy) can be outstanding
    /// together; the clipboard has one request at a time, and a newer one
    /// supersedes it.
    fn concurrent(self) -> bool {
        matches!(self, Self::Sent | Self::Capabilities)
    }
}
/// Most nonces awaited at once for a concurrent operation.
const MAX_PENDING: usize = 16;

/// What the page posted: a short content kind (a MIME type, `none` or
/// `application/json`) and the raw body.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Reply {
    pub kind: String,
    pub body: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Rejection {
    /// Rust has not asked this operation of the page.
    Unsolicited,
    WrongNonce,
    Expired,
}

struct Pending {
    nonce: String,
    expires: Instant,
    sender: mpsc::SyncSender<Reply>,
}

/// Pending single-use nonces for one viewer connection.
#[derive(Default)]
pub(crate) struct Inbox {
    pending: Mutex<HashMap<Op, Vec<Pending>>>,
    rejected: AtomicU64,
}

/// The caller's side of an issued nonce.
pub(crate) struct Expectation {
    pub nonce: String,
    receiver: mpsc::Receiver<Reply>,
}
impl Expectation {
    /// Waits for the page's answer. `Err` means the page never answered with the
    /// nonce, or its answer was refused.
    pub(crate) fn wait(self, timeout: Duration) -> Result<Reply, String> {
        self.receiver
            .recv_timeout(timeout)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => "The computer did not answer.".to_string(),
                mpsc::RecvTimeoutError::Disconnected => {
                    "The computer's answer was not accepted.".to_string()
                }
            })
    }
}

/// A validated, consumed nonce: the right to deliver one body of up to
/// `max_bytes`.
pub(crate) struct Claim {
    pub max_bytes: usize,
    sender: mpsc::SyncSender<Reply>,
}
impl Claim {
    pub(crate) fn deliver(self, reply: Reply) {
        let _ = self.sender.send(reply);
    }
}

impl Inbox {
    /// Issues a nonce for `op`, valid for `ttl`. A newer request for the same
    /// operation supersedes an older one.
    pub(crate) fn expect(&self, op: Op, ttl: Duration) -> Expectation {
        let nonce = uuid::Uuid::new_v4().simple().to_string();
        let (sender, receiver) = mpsc::sync_channel(1);
        if let Ok(mut pending) = self.pending.lock() {
            let now = Instant::now();
            let entries = pending.entry(op).or_default();
            if op.concurrent() {
                entries.retain(|entry| entry.expires > now);
                if entries.len() >= MAX_PENDING {
                    entries.remove(0);
                }
            } else {
                entries.clear();
            }
            entries.push(Pending {
                nonce: nonce.clone(),
                expires: now + ttl,
                sender,
            });
        }
        Expectation { nonce, receiver }
    }

    /// Consumes the pending nonce for `op` when `nonce` matches and has not
    /// expired. A wrong nonce leaves the real one in place, so the page cannot
    /// cancel a request by guessing.
    pub(crate) fn claim(&self, op: Op, nonce: &str, now: Instant) -> Result<Claim, Rejection> {
        let mut pending = self.pending.lock().map_err(|_| Rejection::Unsolicited)?;
        let entries = pending.entry(op).or_default();
        let Some(index) = entries.iter().position(|entry| entry.nonce == nonce) else {
            entries.retain(|entry| entry.expires > now);
            return Err(if entries.is_empty() {
                Rejection::Unsolicited
            } else {
                Rejection::WrongNonce
            });
        };
        let entry = entries.remove(index);
        if entry.expires <= now {
            return Err(Rejection::Expired);
        }
        Ok(Claim {
            max_bytes: op.max_bytes(),
            sender: entry.sender,
        })
    }

    fn log_rejection(&self, what: &str) {
        let count = self.rejected.fetch_add(1, Ordering::Relaxed);
        if count < 5 || count.is_multiple_of(100) {
            eprintln!("Silo desktop bridge: rejected request ({what}); {count} earlier");
        }
    }
}

/// HTTP status for a request on the reserved route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Accepted,
    BadRequest,
    Forbidden,
    NotFound,
    TooLarge,
}
impl Status {
    pub(crate) fn line(self) -> &'static str {
        match self {
            Self::Accepted => "204 No Content",
            Self::BadRequest => "400 Bad Request",
            Self::Forbidden => "403 Forbidden",
            Self::NotFound => "404 Not Found",
            Self::TooLarge => "413 Content Too Large",
        }
    }
}

/// Decodes `%XX` escapes strictly: a malformed escape is `None`.
fn percent_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let pair = bytes.get(index + 1..index + 3)?;
            if !pair.iter().all(u8::is_ascii_hexdigit) {
                return None;
            }
            let value = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
            out.push(value);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    Some(out)
}

/// The path as a server behind the proxy could read it: escapes (repeatedly
/// decoded), backslashes, case, repeated slashes, dot segments and path
/// parameters are all resolved.
fn canonical_path(target: &str) -> String {
    let mut path = target
        .split(['?', '#'])
        .next()
        .unwrap_or(target)
        .to_string();
    for _ in 0..4 {
        let decoded = String::from_utf8_lossy(&percent_decode_lossy(&path)).into_owned();
        if decoded == path {
            break;
        }
        path = decoded;
    }
    let mut segments: Vec<&str> = Vec::new();
    let lowered = path.replace('\\', "/").to_ascii_lowercase();
    for segment in lowered.split('/') {
        let segment = segment.split(';').next().unwrap_or("").trim();
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    format!("/{}", segments.join("/"))
}

/// Like `percent_decode`, but a malformed escape stays literal.
fn percent_decode_lossy(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escape = (bytes[index] == b'%')
            .then(|| bytes.get(index + 1..index + 3))
            .flatten()
            .filter(|pair| pair.iter().all(u8::is_ascii_hexdigit))
            .and_then(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok());
        match escape {
            Some(value) => {
                out.push(value);
                index += 3;
            }
            None => {
                out.push(bytes[index]);
                index += 1;
            }
        }
    }
    out
}

fn in_reserved_namespace(path: &str) -> bool {
    path == "/__silo" || path.starts_with("/__silo/")
}

/// True for every target the proxy must answer itself and never forward: the
/// reserved namespace under any spelling a server behind the proxy could
/// resolve to it.
pub(crate) fn is_reserved(target: &str) -> bool {
    let path = target.split(['?', '#']).next().unwrap_or(target);
    in_reserved_namespace(path) || in_reserved_namespace(&canonical_path(target))
}

struct Route {
    op: Op,
    nonce: String,
    kind: String,
}
/// Longest raw query value; escapes triple the size of the decoded value.
const MAX_QUERY_VALUE: usize = 192;
fn query_value(raw: &str) -> Result<String, Status> {
    if raw.len() > MAX_QUERY_VALUE {
        return Err(Status::BadRequest);
    }
    let decoded = percent_decode(raw).ok_or(Status::BadRequest)?;
    String::from_utf8(decoded).map_err(|_| Status::BadRequest)
}
fn parse_route(target: &str) -> Result<Route, Status> {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    // Only the exact spelling of a handled route is served; an alias of the
    // reserved namespace is refused rather than answered.
    if path != canonical_path(path) {
        return Err(Status::Forbidden);
    }
    let name = path.strip_prefix(ROUTE_PREFIX).ok_or(Status::NotFound)?;
    let op = Op::parse(name).ok_or(Status::NotFound)?;
    let (mut nonce, mut kind) = (None, None);
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("nonce", value)) if nonce.is_none() => nonce = Some(query_value(value)?),
            Some(("kind", value)) if kind.is_none() => kind = Some(query_value(value)?),
            _ => return Err(Status::BadRequest),
        }
    }
    let nonce = nonce
        .filter(|n| !n.is_empty() && n.len() <= 64 && n.bytes().all(|b| b.is_ascii_alphanumeric()))
        .ok_or(Status::BadRequest)?;
    let kind = kind
        .filter(|k| {
            !k.is_empty()
                && k.len() <= 64
                && k.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'+' | b'-'))
        })
        .ok_or(Status::BadRequest)?;
    Ok(Route { op, nonce, kind })
}

/// Handles one authenticated request on the reserved route. `read_body` reads
/// exactly the requested number of body bytes and is called only after the
/// nonce is accepted and the declared length is within the operation's cap.
pub(crate) fn serve_route(
    inbox: &Inbox,
    method: &str,
    target: &str,
    content_length: Option<u64>,
    read_body: impl FnOnce(usize) -> std::io::Result<Vec<u8>>,
    now: Instant,
) -> Status {
    let route = match parse_route(target) {
        Ok(route) => route,
        Err(status) => {
            inbox.log_rejection("unknown route");
            return status;
        }
    };
    if method != "POST" {
        inbox.log_rejection("method");
        return Status::BadRequest;
    }
    let claim = match inbox.claim(route.op, &route.nonce, now) {
        Ok(claim) => claim,
        Err(reason) => {
            inbox.log_rejection(&format!("{} {reason:?}", route.op.name()));
            return Status::Forbidden;
        }
    };
    let Some(length) = content_length else {
        inbox.log_rejection("missing length");
        return Status::BadRequest;
    };
    if length > claim.max_bytes as u64 {
        inbox.log_rejection("oversize");
        return Status::TooLarge;
    }
    match read_body(length as usize) {
        Ok(body) => {
            claim.deliver(Reply {
                kind: route.kind,
                body,
            });
            Status::Accepted
        }
        Err(_) => Status::BadRequest,
    }
}

/// Anything that can run script in the guest page.
pub(crate) trait Page {
    fn eval(&self, script: &str) -> Result<(), String>;
}
impl<R: tauri::Runtime> Page for tauri::Webview<R> {
    fn eval(&self, script: &str) -> Result<(), String> {
        tauri::Webview::eval(self, script).map_err(|_| "The desktop display is unavailable.".into())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum GuestClipboard {
    Empty,
    Text(String),
    Image {
        mime: String,
        bytes: Vec<u8>,
    },
    /// The computer announced a selection above the page's cap.
    TooLarge,
}

/// Selkies' multi-flavour clipboard payload (input_handler.py
/// `CLIPBOARD_FLAVOURS_MIME`): a JSON object of MIME type to text.
const FLAVOURS_MIME: &str = "application/x-selkies-clipboard-flavours";

#[derive(Deserialize)]
struct Flavours {
    #[serde(rename = "text/plain", default)]
    plain: Option<String>,
}

/// The plain text of a flavours payload; markup and any other flavour are
/// ignored. `None` when the payload is not a JSON object.
fn flavours_text(body: &[u8]) -> Option<String> {
    // A struct would also accept a JSON array.
    if body.trim_ascii_start().first() != Some(&b'{') {
        return None;
    }
    serde_json::from_slice::<Flavours>(body)
        .ok()
        .map(|flavours| flavours.plain.unwrap_or_default())
}

#[derive(Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub(crate) struct Capabilities {
    #[serde(rename = "audioDecoder")]
    pub audio_decoder: bool,
    /// The web engine can decode 48 kHz stereo Opus, which Selkies audio needs.
    pub opus: bool,
    /// The Selkies WebSocket transport exists and is open. It is absent when
    /// the client runs in WebRTC mode, where none of the bridge's frames work.
    pub transport: bool,
}

/// What the Selkies server's settings, as the client mirrors them, allow for the
/// clipboard. Each field is `None` until the settings have arrived.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub(crate) struct ClipboardPolicy {
    /// The transport is open.
    pub transport: bool,
    /// The server has clipboard transfer turned on at all.
    pub clipboard: Option<bool>,
    /// The server accepts clipboard writes from the client (Paste).
    #[serde(rename = "clipboardIn")]
    pub clipboard_in: Option<bool>,
    /// The server sends its clipboard to the client (Copy).
    #[serde(rename = "clipboardOut")]
    pub clipboard_out: Option<bool>,
}

/// Whether one clipboard direction is available on the computer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClipboardSupport {
    Supported,
    /// The server has it turned off, as on a desktop older than recipe 3.
    Unsupported,
    /// The server allows the clipboard but not this direction.
    Disabled,
    /// The page has not reported the server settings (or is not connected).
    Unknown,
}

impl ClipboardPolicy {
    /// The state of Paste (`writing`) or Copy.
    pub(crate) fn support(self, writing: bool) -> ClipboardSupport {
        let direction = if writing {
            self.clipboard_in
        } else {
            self.clipboard_out
        };
        match (self.clipboard, direction) {
            (Some(false), _) => ClipboardSupport::Unsupported,
            (Some(true), Some(false)) => ClipboardSupport::Disabled,
            (Some(true), Some(true)) if self.transport => ClipboardSupport::Supported,
            _ => ClipboardSupport::Unknown,
        }
    }
}

/// Silo's operations on one viewer's guest page.
pub(crate) struct Bridge<'a> {
    pub page: &'a dyn Page,
    pub inbox: &'a Inbox,
}

fn script(method: &str, args: Value) -> String {
    format!(
        "window.__silo&&window.__silo.invoke({},{});",
        json!(method),
        args
    )
}

fn text_frames(text: &str, id: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    if bytes.len() <= CHUNK_BYTES {
        return vec![format!("cw,{}", STANDARD.encode(bytes))];
    }
    let mut frames = vec![format!("cws,{id},{}", bytes.len())];
    frames.extend(
        bytes
            .chunks(CHUNK_BYTES)
            .map(|chunk| format!("cwd,{id},{}", STANDARD.encode(chunk))),
    );
    frames.push(format!("cwe,{id}"));
    frames
}

fn image_frames(mime: &str, bytes: &[u8], id: &str) -> Vec<String> {
    if bytes.len() <= CHUNK_BYTES {
        return vec![format!("cb,{mime},{}", STANDARD.encode(bytes))];
    }
    let mut frames = vec![format!("cbs,{id},{mime},{}", bytes.len())];
    frames.extend(
        bytes
            .chunks(CHUNK_BYTES)
            .map(|chunk| format!("cbd,{id},{}", STANDARD.encode(chunk))),
    );
    frames.push(format!("cbe,{id}"));
    frames
}

fn chord(key: u32) -> Vec<String> {
    vec![
        format!("kd,{KEYSYM_CONTROL_L}"),
        format!("kd,{key}"),
        format!("ku,{key}"),
        format!("ku,{KEYSYM_CONTROL_L}"),
    ]
}

fn image_mime(mime: &str) -> bool {
    matches!(
        mime,
        "image/png" | "image/jpeg" | "image/webp" | "image/bmp"
    )
}

impl Bridge<'_> {
    fn invoke(&self, method: &str, args: Value) -> Result<(), String> {
        self.page.eval(&script(method, args))
    }

    fn send_frames(&self, frames: &[String]) -> Result<(), String> {
        self.send_acknowledged("sendFrames", frames)
    }

    /// Runs the page's `method` (`sendFrames`, `sendShortcut` or `resetScreen`) and waits for
    /// its acknowledgement, which says whether the socket took every frame.
    fn send_acknowledged(&self, method: &str, frames: &[String]) -> Result<(), String> {
        let expectation = self.inbox.expect(Op::Sent, SEND_WAIT + REPLY_GRACE);
        self.invoke(method, json!([frames, expectation.nonce]))?;
        match expectation.wait(SEND_WAIT)?.kind.as_str() {
            "ok" => Ok(()),
            "closed" => Err(NOT_CONNECTED.into()),
            _ => Err("The computer's display refused the request.".into()),
        }
    }

    /// Sets the computer's clipboard to `text` (Selkies `cw`, or `cws`/`cwd`/`cwe`
    /// above one chunk). The server awaits the write before the next frame.
    pub(crate) fn send_guest_text(&self, text: &str) -> Result<(), String> {
        if text.len() > MAX_TEXT_BYTES {
            return Err("That text is too large to paste into the computer.".into());
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        self.send_frames(&text_frames(text, &id))
    }

    /// Sets the computer's clipboard to an encoded image (Selkies `cb`, or
    /// `cbs`/`cbd`/`cbe` above one chunk).
    pub(crate) fn send_guest_image(&self, mime: &str, bytes: &[u8]) -> Result<(), String> {
        if !image_mime(mime) {
            return Err("That image format cannot be pasted into the computer.".into());
        }
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err("That image is too large to paste into the computer.".into());
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        self.send_frames(&image_frames(mime, bytes, &id))
    }

    /// Presses Ctrl+V in the computer, with the modifiers the page holds down
    /// released first.
    pub(crate) fn press_guest_paste(&self) -> Result<(), String> {
        self.send_acknowledged("sendShortcut", &chord(KEYSYM_V))
    }

    /// Presses Ctrl+C in the computer, without reading the result.
    pub(crate) fn press_guest_copy(&self) -> Result<(), String> {
        self.send_acknowledged("sendShortcut", &chord(KEYSYM_C))
    }

    /// Reads the computer's clipboard: waits up to `timeout` for the next
    /// announcement (optionally after pressing Ctrl+C), then falls back to the
    /// last one the page saw. The guest page cannot send it unprompted.
    pub(crate) fn request_guest_clipboard(
        &self,
        timeout: Duration,
        press_copy: bool,
    ) -> Result<GuestClipboard, String> {
        let timeout = timeout.min(MAX_WAIT);
        let wait = timeout + REPLY_GRACE;
        let expectation = self.inbox.expect(Op::Clipboard, wait);
        let mut frames = if press_copy { chord(KEYSYM_C) } else { vec![] };
        frames.push("REQUEST_CLIPBOARD".into());
        self.invoke(
            "requestClipboard",
            json!([
                expectation.nonce,
                timeout.as_millis() as u64,
                frames,
                press_copy
            ]),
        )?;
        let reply = expectation.wait(wait)?;
        match reply.kind.as_str() {
            "none" => Ok(GuestClipboard::Empty),
            "too-large" => Ok(GuestClipboard::TooLarge),
            "disconnected" => Err(NOT_CONNECTED.into()),
            "refused" => Err("The computer's display refused the request.".into()),
            FLAVOURS_MIME => match flavours_text(&reply.body) {
                Some(text) if text.is_empty() => Ok(GuestClipboard::Empty),
                Some(text) => Ok(GuestClipboard::Text(text)),
                None => Err("The computer's clipboard has content Silo cannot copy.".into()),
            },
            "text/plain" if reply.body.is_empty() => Ok(GuestClipboard::Empty),
            "text/plain" => String::from_utf8(reply.body)
                .map(GuestClipboard::Text)
                .map_err(|_| "The computer's clipboard is not text.".into()),
            mime if image_mime(mime) && !reply.body.is_empty() => Ok(GuestClipboard::Image {
                mime: mime.to_string(),
                bytes: reply.body,
            }),
            _ => Err("The computer's clipboard has content Silo cannot copy.".into()),
        }
    }

    /// Asks the page which media features this web engine has.
    pub(crate) fn capabilities(&self, timeout: Duration) -> Result<Capabilities, String> {
        let wait = timeout.min(MAX_WAIT);
        let expectation = self.inbox.expect(Op::Capabilities, wait + REPLY_GRACE);
        self.invoke("capabilities", json!([expectation.nonce]))?;
        let reply = expectation.wait(wait)?;
        serde_json::from_slice(&reply.body).map_err(|_| "Unreadable desktop capabilities.".into())
    }

    /// Reads the clipboard settings the Selkies server announced to the page.
    pub(crate) fn clipboard_policy(&self, timeout: Duration) -> Result<ClipboardPolicy, String> {
        let wait = timeout.min(MAX_WAIT);
        let expectation = self.inbox.expect(Op::Capabilities, wait + REPLY_GRACE);
        self.invoke("capabilities", json!([expectation.nonce]))?;
        let reply = expectation.wait(wait)?;
        serde_json::from_slice(&reply.body).map_err(|_| "Unreadable desktop capabilities.".into())
    }

    /// Selkies `setMute` page message (silences playback, keeps the stream).
    pub(crate) fn set_audio_muted(&self, muted: bool) -> Result<(), String> {
        self.invoke("setMute", json!([muted]))
    }

    pub(crate) fn set_audio_volume(&self, volume: f64) -> Result<(), String> {
        self.invoke("setVolume", json!([volume.clamp(0., 1.)]))
    }

    /// Starts or stops the audio stream through Selkies' `pipelineControl`
    /// page message, so the client and server agree on the pipeline state.
    pub(crate) fn set_audio_active(&self, active: bool) -> Result<(), String> {
        self.invoke("setAudioActive", json!([active]))
    }

    /// Asks the server for a screen size (`r,WxH,primary`) at 96 DPI (`s,96`)
    /// through the page's `resetScreen`, which in the same task lets the next
    /// window resize take the client back to the window's size and density.
    /// Selkies wants even dimensions of at most 4080.
    pub(crate) fn reset_resolution(&self, width: u32, height: u32) -> Result<(), String> {
        if !(16..=MAX_SCREEN_EDGE).contains(&width) || !(16..=MAX_SCREEN_EDGE).contains(&height) {
            return Err("Invalid screen size.".into());
        }
        self.send_acknowledged(
            "resetScreen",
            &[
                format!("r,{}x{},primary", width & !1, height & !1),
                "s,96".to_string(),
            ],
        )
    }

    /// Selkies `resetResolutionToWindow` page message.
    pub(crate) fn reset_resolution_to_window(&self) -> Result<(), String> {
        self.invoke("resetResolutionToWindow", json!([]))
    }
}

/// Fake pages for tests of the bridge and the code built on it.
#[cfg(test)]
pub(crate) mod test_page {
    use super::*;
    use std::cell::{Cell, RefCell};

    /// The method and arguments of a script the bridge evaluates.
    pub(crate) fn invocation(script: &str) -> (String, Vec<Value>) {
        let call = script
            .strip_prefix("window.__silo&&window.__silo.invoke(")
            .and_then(|rest| rest.strip_suffix(");"))
            .expect("a bridge script");
        let (method, args) = call.split_once(',').expect("a method and arguments");
        (
            serde_json::from_str(method).unwrap(),
            serde_json::from_str(args).unwrap(),
        )
    }

    /// Answers a `sendFrames`, `sendShortcut` or `resetScreen` script the way the page does, with `outcome`.
    /// Returns whether the script was a send.
    pub(crate) fn acknowledge_send(inbox: &Inbox, script: &str, outcome: &str) -> bool {
        let (method, args) = invocation(script);
        if !matches!(
            method.as_str(),
            "sendFrames" | "sendShortcut" | "resetScreen"
        ) {
            return false;
        }
        let nonce = args[1].as_str().expect("a nonce");
        inbox
            .claim(Op::Sent, nonce, Instant::now())
            .expect("a pending acknowledgement")
            .deliver(Reply {
                kind: outcome.to_string(),
                body: vec![],
            });
        true
    }

    /// Records scripts and acknowledges sends with a configurable outcome.
    pub(crate) struct AckingPage<'a> {
        pub inbox: &'a Inbox,
        pub outcome: Cell<&'static str>,
        pub scripts: RefCell<Vec<String>>,
    }
    impl<'a> AckingPage<'a> {
        pub(crate) fn new(inbox: &'a Inbox) -> Self {
            Self {
                inbox,
                outcome: Cell::new("ok"),
                scripts: RefCell::default(),
            }
        }
    }
    impl Page for AckingPage<'_> {
        fn eval(&self, script: &str) -> Result<(), String> {
            self.scripts.borrow_mut().push(script.to_string());
            acknowledge_send(self.inbox, script, self.outcome.get());
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_page::*;
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct FakePage {
        scripts: RefCell<Vec<String>>,
    }
    impl Page for FakePage {
        fn eval(&self, script: &str) -> Result<(), String> {
            self.scripts.borrow_mut().push(script.to_string());
            Ok(())
        }
    }

    fn post(inbox: &Inbox, target: &str, body: &[u8]) -> Status {
        serve_route(
            inbox,
            "POST",
            target,
            Some(body.len() as u64),
            |n| Ok(body[..n].to_vec()),
            Instant::now(),
        )
    }

    #[test]
    fn unsolicited_requests_are_rejected() {
        let inbox = Inbox::default();
        let target = format!("{ROUTE_PREFIX}clipboard?nonce=abc&kind=text/plain");
        assert_eq!(post(&inbox, &target, b"hi"), Status::Forbidden);
        // A nonce for another operation does not open this one.
        let other = inbox.expect(Op::Capabilities, Duration::from_secs(5));
        let target = format!(
            "{ROUTE_PREFIX}clipboard?nonce={}&kind=text/plain",
            other.nonce
        );
        assert_eq!(post(&inbox, &target, b"hi"), Status::Forbidden);
    }

    #[test]
    fn a_nonce_works_once() {
        let inbox = Inbox::default();
        let expectation = inbox.expect(Op::Clipboard, Duration::from_secs(5));
        let target = format!(
            "{ROUTE_PREFIX}clipboard?nonce={}&kind=text/plain",
            expectation.nonce
        );
        assert_eq!(post(&inbox, &target, b"hello"), Status::Accepted);
        assert_eq!(post(&inbox, &target, b"again"), Status::Forbidden);
        let reply = expectation.wait(Duration::from_secs(1)).unwrap();
        assert_eq!(reply.kind, "text/plain");
        assert_eq!(reply.body, b"hello");
    }

    #[test]
    fn a_wrong_nonce_neither_succeeds_nor_cancels_the_request() {
        let inbox = Inbox::default();
        let expectation = inbox.expect(Op::Clipboard, Duration::from_secs(5));
        let wrong = format!("{ROUTE_PREFIX}clipboard?nonce=wrong&kind=text/plain");
        assert_eq!(post(&inbox, &wrong, b"x"), Status::Forbidden);
        let right = format!(
            "{ROUTE_PREFIX}clipboard?nonce={}&kind=text/plain",
            expectation.nonce
        );
        assert_eq!(post(&inbox, &right, b"x"), Status::Accepted);
    }

    #[test]
    fn expired_nonces_are_rejected() {
        let inbox = Inbox::default();
        let expectation = inbox.expect(Op::Clipboard, Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(10));
        let target = format!(
            "{ROUTE_PREFIX}clipboard?nonce={}&kind=text/plain",
            expectation.nonce
        );
        assert_eq!(post(&inbox, &target, b"late"), Status::Forbidden);
        assert!(expectation.wait(Duration::from_millis(50)).is_err());
        assert_eq!(
            inbox.claim(Op::Clipboard, "x", Instant::now()).err(),
            Some(Rejection::Unsolicited)
        );
    }

    #[test]
    fn oversize_bodies_are_refused_before_they_are_read() {
        let inbox = Inbox::default();
        let expectation = inbox.expect(Op::Capabilities, Duration::from_secs(5));
        let target = format!(
            "{ROUTE_PREFIX}capabilities?nonce={}&kind=application/json",
            expectation.nonce
        );
        let status = serve_route(
            &inbox,
            "POST",
            &target,
            Some(Op::Capabilities.max_bytes() as u64 + 1),
            |_| panic!("an oversize body must not be read"),
            Instant::now(),
        );
        assert_eq!(status, Status::TooLarge);
        assert!(expectation.wait(Duration::from_millis(50)).is_err());
    }

    #[test]
    fn unknown_operations_methods_and_malformed_queries_are_rejected() {
        let inbox = Inbox::default();
        let expectation = inbox.expect(Op::Clipboard, Duration::from_secs(5));
        let nonce = &expectation.nonce;
        let unknown = format!("{ROUTE_PREFIX}files?nonce={nonce}&kind=text/plain");
        assert_eq!(post(&inbox, &unknown, b""), Status::NotFound);
        assert_eq!(post(&inbox, "/__silo/v2/clipboard", b""), Status::NotFound);
        for target in [
            format!("{ROUTE_PREFIX}clipboard"),
            format!("{ROUTE_PREFIX}clipboard?nonce={nonce}"),
            format!("{ROUTE_PREFIX}clipboard?nonce={nonce}&kind=a b"),
            format!("{ROUTE_PREFIX}clipboard?nonce={nonce}&kind=text/plain&extra=1"),
            format!("{ROUTE_PREFIX}clipboard?nonce={nonce}&nonce={nonce}&kind=text/plain"),
        ] {
            assert_eq!(post(&inbox, &target, b""), Status::BadRequest, "{target}");
        }
        let status = serve_route(
            &inbox,
            "GET",
            &format!("{ROUTE_PREFIX}clipboard?nonce={nonce}&kind=text/plain"),
            None,
            |_| Ok(vec![]),
            Instant::now(),
        );
        assert_eq!(status, Status::BadRequest);
        // None of those consumed the nonce.
        let good = format!("{ROUTE_PREFIX}clipboard?nonce={nonce}&kind=none");
        assert_eq!(post(&inbox, &good, b""), Status::Accepted);
    }

    /// The requests `desktop_viewer_bridge.js` produces, shared with
    /// `desktop/linux-desktop-bridge.test.ts`.
    #[derive(Deserialize)]
    struct ContractCase {
        name: String,
        url: String,
        body: String,
        kind: String,
    }

    #[test]
    fn the_requests_the_page_script_produces_are_accepted() {
        let cases: Vec<ContractCase> =
            serde_json::from_str(include_str!("desktop_bridge_contract.json")).unwrap();
        assert!(!cases.is_empty());
        for case in cases {
            let name = case.url[ROUTE_PREFIX.len()..].split('?').next().unwrap();
            let op = Op::parse(name).expect("a known operation");
            let inbox = Inbox::default();
            let expectation = inbox.expect(op, Duration::from_secs(5));
            let target = case.url.replace("abc123", &expectation.nonce);
            assert_eq!(
                post(&inbox, &target, case.body.as_bytes()),
                Status::Accepted,
                "{}",
                case.name
            );
            let reply = expectation.wait(Duration::from_secs(1)).unwrap();
            assert_eq!(reply.kind, case.kind, "{}", case.name);
            assert_eq!(reply.body, case.body.as_bytes(), "{}", case.name);
        }
    }

    #[test]
    fn query_escapes_are_strict_and_bounded() {
        let inbox = Inbox::default();
        let expectation = inbox.expect(Op::Clipboard, Duration::from_secs(5));
        let n = &expectation.nonce;
        for bad in [
            format!("kind=text%2&nonce={n}"),
            format!("kind=text%zzplain&nonce={n}"),
            format!("kind=text%2Bplain%00&nonce={n}"),
            format!("kind=%2F%2F%25&nonce={n}"),
            format!("kind=text%2Fplain&nonce={n}%"),
            format!("kind={}&nonce={n}", "%41".repeat(70)),
            format!("kind=%ff&nonce={n}"),
        ] {
            let target = format!("{ROUTE_PREFIX}clipboard?{bad}");
            assert_eq!(post(&inbox, &target, b"x"), Status::BadRequest, "{bad}");
        }
        // None of those consumed the nonce.
        let good = format!("{ROUTE_PREFIX}clipboard?nonce={n}&kind=text%2Fplain");
        assert_eq!(post(&inbox, &good, b"x"), Status::Accepted);
    }

    #[test]
    fn aliases_of_the_reserved_namespace_are_reserved_and_refused() {
        let aliases = [
            "/%5f%5fsilo/v1/clipboard?nonce=a&kind=none",
            "/%5F%5Fsilo/v1/clipboard",
            "/__SILO/v1/clipboard",
            "/__Silo/v1/clipboard",
            "/__silo//v1/clipboard",
            "/__silo/./v1/clipboard",
            "/x/../__silo/v1/clipboard",
            "/x/%2e%2e/__silo/v1/clipboard",
            "/%2e/__silo/v1/clipboard",
            "/__silo/v1/../v1/clipboard",
            "/__silo/%76%31/clipboard",
            "/__silo/v1%2Fclipboard",
            "/%255f%255fsilo/v1/clipboard",
            "/%2F__silo/v1/clipboard",
            "/\\__silo/v1/clipboard",
            "/__silo;x/v1/clipboard",
            "/__silo/v1/clipboard/",
        ];
        let inbox = Inbox::default();
        let expectation = inbox.expect(Op::Clipboard, Duration::from_secs(5));
        for alias in aliases {
            assert!(is_reserved(alias), "{alias}");
            let status = serve_route(
                &inbox,
                "POST",
                alias,
                Some(1),
                |_| Ok(vec![0]),
                Instant::now(),
            );
            assert_eq!(status, Status::Forbidden, "{alias}");
        }
        // Aliases did not spend the nonce.
        let target = format!(
            "{ROUTE_PREFIX}clipboard?nonce={}&kind=none",
            expectation.nonce
        );
        assert_eq!(post(&inbox, &target, b""), Status::Accepted);
        for ordinary in [
            "/",
            "/index.html",
            "/%5f%5fsilox",
            "/a/__silo/v1/clipboard",
            "/websockify",
        ] {
            assert!(!is_reserved(ordinary), "{ordinary}");
        }
    }

    #[test]
    fn every_reserved_path_is_recognised() {
        for target in [
            "/__silo",
            "/__silo/",
            "/__silo/v1/x?y=1",
            "/__silo/../a",
            "/__silo?x",
        ] {
            assert!(is_reserved(target), "{target}");
        }
        for target in ["/", "/__silox", "/a/__silo/v1/clipboard", "/websockify"] {
            assert!(!is_reserved(target), "{target}");
        }
    }

    #[test]
    fn text_is_chunked_like_selkies() {
        assert_eq!(text_frames("hi", "id"), ["cw,aGk="]);
        let big = "é".repeat(CHUNK_BYTES);
        let frames = text_frames(&big, "id");
        assert_eq!(frames[0], format!("cws,id,{}", big.len()));
        assert_eq!(frames.last().unwrap(), "cwe,id");
        let mut joined = Vec::new();
        for frame in &frames[1..frames.len() - 1] {
            let data = frame.strip_prefix("cwd,id,").unwrap();
            joined.extend(STANDARD.decode(data).unwrap());
        }
        assert_eq!(joined, big.as_bytes());
        assert!(frames[1..frames.len() - 1].iter().all(|f| f.len() < 25_000));
    }

    #[test]
    fn images_carry_their_mime_and_chunk_with_a_size_header() {
        assert_eq!(
            image_frames("image/png", b"abc", "id"),
            ["cb,image/png,YWJj"]
        );
        let bytes = vec![7u8; CHUNK_BYTES * 2 + 1];
        let frames = image_frames("image/png", &bytes, "id");
        assert_eq!(frames[0], format!("cbs,id,image/png,{}", bytes.len()));
        assert_eq!(frames.len(), 5);
        assert_eq!(frames[4], "cbe,id");
    }

    #[test]
    fn payloads_reach_the_page_only_as_json() {
        let inbox = Inbox::default();
        let page = AckingPage::new(&inbox);
        let bridge = Bridge {
            page: &page,
            inbox: &inbox,
        };
        let hostile = "\"); alert(1); (\"\u{2028}`${x}`\\";
        bridge.send_guest_text(hostile).unwrap();
        bridge.press_guest_paste().unwrap();
        bridge.set_audio_muted(true).unwrap();
        let scripts = page.scripts.borrow();
        assert_eq!(scripts.len(), 3);
        // The text is base64 inside a JSON array; nothing of it appears raw.
        assert!(!scripts[0].contains("alert"));
        assert!(scripts[0].starts_with("window.__silo&&window.__silo.invoke(\"sendFrames\",[["));
        let (method, args) = invocation(&scripts[1]);
        assert_eq!(method, "sendShortcut");
        assert_eq!(args[0], json!(["kd,65507", "kd,118", "ku,118", "ku,65507"]));
        assert!(args[1].as_str().is_some());
        assert_eq!(
            scripts[2],
            "window.__silo&&window.__silo.invoke(\"setMute\",[true]);"
        );
    }

    #[test]
    fn oversize_text_images_and_bad_formats_are_refused_locally() {
        let inbox = Inbox::default();
        let page = AckingPage::new(&inbox);
        let bridge = Bridge {
            page: &page,
            inbox: &inbox,
        };
        assert!(bridge
            .send_guest_text(&"x".repeat(MAX_TEXT_BYTES + 1))
            .is_err());
        assert!(bridge
            .send_guest_image("image/png", &vec![0; MAX_IMAGE_BYTES + 1])
            .is_err());
        assert!(bridge.send_guest_image("text/html", b"<b>").is_err());
        assert!(bridge.send_guest_image("image/png,x", b"a").is_err());
        assert!(bridge.reset_resolution(0, 900).is_err());
        assert!(bridge.reset_resolution(1440, 100_000).is_err());
        assert!(page.scripts.borrow().is_empty());
        assert!(bridge.reset_resolution(4082, 900).is_err());
        bridge.reset_resolution(1441, 901).unwrap();
        bridge.set_audio_active(true).unwrap();
        let scripts = page.scripts.borrow();
        let (method, args) = invocation(&scripts[0]);
        assert_eq!(method, "resetScreen");
        assert_eq!(args[0], json!(["r,1440x900,primary", "s,96"]));
        assert!(scripts[1].ends_with("invoke(\"setAudioActive\",[true]);"));
    }

    /// A page that answers the nonce in the eval'd call, like the real helper.
    struct AnsweringPage<'a> {
        inbox: &'a Inbox,
        op: Op,
        kind: &'a str,
        body: &'a [u8],
        seen: RefCell<Vec<String>>,
    }
    impl Page for AnsweringPage<'_> {
        fn eval(&self, script: &str) -> Result<(), String> {
            self.seen.borrow_mut().push(script.to_string());
            let start = script.find("[\"").ok_or("no nonce")? + 2;
            let nonce: String = script[start..].chars().take_while(|c| *c != '"').collect();
            let target = format!(
                "{ROUTE_PREFIX}{}?nonce={nonce}&kind={}",
                self.op.name(),
                self.kind
            );
            assert_eq!(post(self.inbox, &target, self.body), Status::Accepted);
            Ok(())
        }
    }

    #[test]
    fn guest_clipboard_requests_decode_text_and_images_and_refuse_the_rest() {
        let cases: [(&str, &[u8], Result<GuestClipboard, ()>); 6] = [
            (
                "text/plain",
                "héllo".as_bytes(),
                Ok(GuestClipboard::Text("héllo".into())),
            ),
            ("none", b"", Ok(GuestClipboard::Empty)),
            ("text/plain", b"", Ok(GuestClipboard::Empty)),
            (
                "image/png",
                b"\x89PNG",
                Ok(GuestClipboard::Image {
                    mime: "image/png".into(),
                    bytes: b"\x89PNG".to_vec(),
                }),
            ),
            ("text/html", b"<b>", Err(())),
            ("text/plain", &[0xff, 0xfe], Err(())),
        ];
        for (kind, body, expected) in cases {
            let inbox = Inbox::default();
            let page = AnsweringPage {
                inbox: &inbox,
                op: Op::Clipboard,
                kind,
                body,
                seen: RefCell::default(),
            };
            let bridge = Bridge {
                page: &page,
                inbox: &inbox,
            };
            let result = bridge.request_guest_clipboard(Duration::from_millis(500), true);
            assert_eq!(result.map_err(|_| ()), expected, "{kind}");
            let script = page.seen.borrow()[0].clone();
            assert!(script.contains("\"requestClipboard\""));
            assert!(script
                .contains("\"kd,65507\",\"kd,99\",\"ku,99\",\"ku,65507\",\"REQUEST_CLIPBOARD\""));
        }
    }

    #[test]
    fn capabilities_are_read_from_the_pages_answer() {
        let inbox = Inbox::default();
        let page = AnsweringPage {
            inbox: &inbox,
            op: Op::Capabilities,
            kind: "application/json",
            body: br#"{"audioDecoder":true,"opus":false,"transport":true,"extra":1}"#,
            seen: RefCell::default(),
        };
        let bridge = Bridge {
            page: &page,
            inbox: &inbox,
        };
        let capabilities = bridge.capabilities(Duration::from_secs(1)).unwrap();
        assert_eq!(
            capabilities,
            Capabilities {
                audio_decoder: true,
                opus: false,
                transport: true
            }
        );
    }

    #[test]
    fn overlapping_capability_questions_are_each_answered() {
        let inbox = Inbox::default();
        let first = inbox.expect(Op::Capabilities, Duration::from_secs(5));
        let second = inbox.expect(Op::Capabilities, Duration::from_secs(5));
        let target =
            |nonce: &str| format!("{ROUTE_PREFIX}capabilities?nonce={nonce}&kind=application/json");
        assert_eq!(
            post(&inbox, &target(&second.nonce), b"{}"),
            Status::Accepted
        );
        assert_eq!(post(&inbox, &target(&first.nonce), b"{}"), Status::Accepted);
        assert!(first.wait(Duration::from_secs(1)).is_ok());
        assert!(second.wait(Duration::from_secs(1)).is_ok());
    }

    #[test]
    fn a_newer_clipboard_request_supersedes_an_older_one() {
        let inbox = Inbox::default();
        let old = inbox.expect(Op::Clipboard, Duration::from_secs(5));
        let new = inbox.expect(Op::Clipboard, Duration::from_secs(5));
        assert!(inbox
            .claim(Op::Clipboard, &old.nonce, Instant::now())
            .is_err());
        assert!(inbox
            .claim(Op::Clipboard, &new.nonce, Instant::now())
            .is_ok());
    }

    #[test]
    fn a_send_succeeds_only_when_the_page_says_the_socket_took_it() {
        let inbox = Inbox::default();
        let page = AckingPage::new(&inbox);
        let bridge = Bridge {
            page: &page,
            inbox: &inbox,
        };
        assert_eq!(bridge.send_guest_text("hi"), Ok(()));
        page.outcome.set("closed");
        assert_eq!(
            bridge.send_guest_text("hi"),
            Err("The desktop is not connected.".to_string())
        );
        assert_eq!(
            bridge.press_guest_paste(),
            Err("The desktop is not connected.".to_string())
        );
        page.outcome.set("refused");
        assert!(bridge.press_guest_copy().is_err());
    }

    #[test]
    fn a_send_the_page_never_acknowledges_fails() {
        let page = FakePage::default();
        let inbox = Inbox::default();
        let bridge = Bridge {
            page: &page,
            inbox: &inbox,
        };
        let started = Instant::now();
        assert!(bridge.send_guest_text("hi").is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn acknowledgements_for_several_sends_can_be_pending_together() {
        let inbox = Inbox::default();
        let first = inbox.expect(Op::Sent, Duration::from_secs(5));
        let second = inbox.expect(Op::Sent, Duration::from_secs(5));
        let target = |nonce: &str| format!("{ROUTE_PREFIX}sent?nonce={nonce}&kind=ok");
        assert_eq!(post(&inbox, &target(&second.nonce), b""), Status::Accepted);
        assert_eq!(post(&inbox, &target(&first.nonce), b""), Status::Accepted);
        assert_eq!(post(&inbox, &target(&first.nonce), b""), Status::Forbidden);
        assert_eq!(first.wait(Duration::from_secs(1)).unwrap().kind, "ok");
        assert_eq!(second.wait(Duration::from_secs(1)).unwrap().kind, "ok");
        // An acknowledgement carries no body.
        let third = inbox.expect(Op::Sent, Duration::from_secs(5));
        assert_eq!(post(&inbox, &target(&third.nonce), b"x"), Status::TooLarge);
    }

    #[test]
    fn clipboard_answers_cover_flavours_oversize_and_a_closed_transport() {
        let html_and_text = br#"{"text/html":"<b>hi</b>","text/plain":"hi","other":1}"#;
        let html_only = br#"{"text/html":"<b>hi</b>"}"#;
        let cases: [(&str, &[u8], Result<GuestClipboard, ()>); 8] = [
            (
                FLAVOURS_MIME,
                html_and_text,
                Ok(GuestClipboard::Text("hi".into())),
            ),
            (FLAVOURS_MIME, html_only, Ok(GuestClipboard::Empty)),
            (FLAVOURS_MIME, b"not json", Err(())),
            (FLAVOURS_MIME, b"[\"text/plain\"]", Err(())),
            ("too-large", b"", Ok(GuestClipboard::TooLarge)),
            ("disconnected", b"", Err(())),
            ("refused", b"", Err(())),
            ("unreadable", b"", Err(())),
        ];
        for (kind, body, expected) in cases {
            let inbox = Inbox::default();
            let page = AnsweringPage {
                inbox: &inbox,
                op: Op::Clipboard,
                kind,
                body,
                seen: RefCell::default(),
            };
            let bridge = Bridge {
                page: &page,
                inbox: &inbox,
            };
            let result = bridge.request_guest_clipboard(Duration::from_millis(500), false);
            assert_eq!(result.map_err(|_| ()), expected, "{kind}");
        }
        let inbox = Inbox::default();
        let page = AnsweringPage {
            inbox: &inbox,
            op: Op::Clipboard,
            kind: "disconnected",
            body: b"",
            seen: RefCell::default(),
        };
        let bridge = Bridge {
            page: &page,
            inbox: &inbox,
        };
        assert_eq!(
            bridge
                .request_guest_clipboard(Duration::from_millis(500), true)
                .unwrap_err(),
            "The desktop is not connected."
        );
    }

    #[test]
    fn the_clipboard_policy_distinguishes_off_unknown_and_supported() {
        let policy = |json: &str| serde_json::from_str::<ClipboardPolicy>(json).unwrap();
        let supported =
            policy(r#"{"transport":true,"clipboard":true,"clipboardIn":true,"clipboardOut":true}"#);
        assert_eq!(supported.support(true), ClipboardSupport::Supported);
        assert_eq!(supported.support(false), ClipboardSupport::Supported);
        let off = policy(
            r#"{"transport":true,"clipboard":false,"clipboardIn":false,"clipboardOut":false}"#,
        );
        assert_eq!(off.support(true), ClipboardSupport::Unsupported);
        assert_eq!(off.support(false), ClipboardSupport::Unsupported);
        let one_way = policy(
            r#"{"transport":true,"clipboard":true,"clipboardIn":false,"clipboardOut":true}"#,
        );
        assert_eq!(one_way.support(true), ClipboardSupport::Disabled);
        assert_eq!(one_way.support(false), ClipboardSupport::Supported);
        // Settings the page has not received, or a closed transport, are unknown.
        let pending =
            policy(r#"{"transport":true,"clipboard":null,"clipboardIn":null,"clipboardOut":null}"#);
        assert_eq!(pending.support(true), ClipboardSupport::Unknown);
        assert_eq!(pending.support(false), ClipboardSupport::Unknown);
        assert_eq!(policy("{}").support(true), ClipboardSupport::Unknown);
        let closed = policy(
            r#"{"transport":false,"clipboard":true,"clipboardIn":true,"clipboardOut":true}"#,
        );
        assert_eq!(closed.support(true), ClipboardSupport::Unknown);
    }

    #[test]
    fn a_silent_page_times_out() {
        let page = FakePage::default();
        let inbox = Inbox::default();
        let bridge = Bridge {
            page: &page,
            inbox: &inbox,
        };
        let started = Instant::now();
        let error = bridge
            .request_guest_clipboard(Duration::from_millis(1), false)
            .unwrap_err();
        // The wait is the request plus the reply grace, never unbounded.
        assert!(started.elapsed() < Duration::from_secs(8));
        assert!(!error.is_empty());
    }
}
