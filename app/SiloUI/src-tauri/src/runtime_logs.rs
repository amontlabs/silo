//! Bounded, rotation-aware queries over all retained diagnostic files.
use super::*;
use crate::bridge_error::{BridgeError, ErrorCode};
use std::io::{BufRead, BufReader};
use std::os::unix::fs::MetadataExt;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Query {
    pub computer_id: String,
    pub device_id: Option<String>,
    pub query: Option<String>,
    pub source: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
    pub around_id: Option<String>,
    /// Snapshot of the previous first page: a Follow refresh reads only what was
    /// appended since. Older hosts ignore it and run a full query.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Entry {
    pub id: String,
    pub line: String,
    pub occurred_at: String,
    pub computer_id: String,
    pub computer_name: String,
    pub device_id: String,
    pub device_name: String,
    pub source: String,
    pub session: Option<String>,
    /// The timestamp was parsed from console text the guest wrote, so the guest chose it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub guest_timestamp: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Page {
    pub entries: Vec<Entry>,
    pub next_cursor: Option<String>,
    pub oldest_available_timestamp: Option<String>,
    pub newest_available_timestamp: Option<String>,
    pub total_matches: usize,
    pub timestamp_estimated: bool,
    /// The owning device runs a Silo that cannot serve logs. Older devices never send this.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unsupported: bool,
    /// Some records were malformed or over the size limit and are shown as placeholders
    /// or truncated. Older devices never send this.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unreadable_records: bool,
    /// Snapshot this page came from, for the next Follow refresh. Older devices never send this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Segment {
    inode: u64,
    bytes: u64,
    stream: String,
    modified: String,
}
struct Location {
    file: u64,
    offset: u64,
    in_pem: bool,
    time: String,
    id: String,
}
/// One retained file as indexed by a snapshot.
struct Indexed {
    segment: Segment,
    /// Records before this offset end with a newline. A follow refresh reads from
    /// here: a final record still being written is read again once complete.
    consumed: u64,
    /// Coverage of the records before `consumed`.
    complete: Summary,
}
struct Cached {
    binding: String,
    files: Vec<Indexed>,
    records: Vec<Location>,
    redaction: Redaction,
    summary: Summary,
}
type Cache = std::sync::Mutex<HashMap<String, (Instant, std::sync::Arc<Cached>)>>;
#[cfg(not(test))]
static CACHE: std::sync::OnceLock<Cache> = std::sync::OnceLock::new();
#[cfg(not(test))]
fn cache() -> &'static Cache {
    CACHE.get_or_init(Default::default)
}
// Each test thread gets its own cache so parallel tests cannot evict each other's snapshots.
#[cfg(test)]
fn cache() -> &'static Cache {
    thread_local! {
        static CACHE: &'static Cache = Box::leak(Box::default());
    }
    CACHE.with(|cache| *cache)
}

/// PEM state belongs to a stream and execution session, independent of search filters.
#[derive(Clone)]
struct Redaction {
    sessions: HashSet<(String, Option<String>)>,
    /// Estimated memory held by `sessions`.
    bytes: usize,
    /// Estimated memory held by the matching records collected beside `sessions`.
    records: usize,
    limit: usize,
}
impl Default for Redaction {
    fn default() -> Self {
        Self {
            sessions: HashSet::new(),
            bytes: 0,
            records: 0,
            limit: INDEX_BUDGET,
        }
    }
}
impl Redaction {
    /// Twice the owned size covers the table's spare capacity and control bytes.
    fn session_cost(key: &(String, Option<String>)) -> usize {
        2 * (std::mem::size_of::<(String, Option<String>)>()
            + key.0.len()
            + key.1.as_ref().map_or(0, String::len))
    }
    fn apply(&mut self, stream: &str, decoded: &mut Decoded) -> Result<(), String> {
        let key = (stream.to_owned(), decoded.session.clone());
        decoded.in_pem = self.sessions.contains(&key);
        let mut in_pem = decoded.in_pem;
        decoded.body = runtime_activity::log_text_with_pem(&decoded.body, &mut in_pem);
        if in_pem {
            if !decoded.in_pem {
                let cost = Self::session_cost(&key);
                // Dropping an open session would expose the rest of its block.
                if self.bytes + self.records + cost > self.limit {
                    return Err(TOO_MANY_MATCHES.into());
                }
                self.bytes += cost;
                self.sessions.insert(key);
            }
        } else if self.sessions.remove(&key) {
            self.bytes -= Self::session_cost(&key);
        }
        Ok(())
    }
}

// The runtime writes this as one atomic, pretty-printed JSON document, not JSONL.
fn boot_record(raw: &str) -> Result<(String, String), String> {
    #[derive(Deserialize)]
    struct BootError {
        t: String,
        stage: String,
        errno: Option<i32>,
        message: String,
    }
    let error: BootError = serde_json::from_str(raw)
        .map_err(|_| "The retained boot failure contains invalid data.")?;
    let errno = error
        .errno
        .map(|value| format!(", errno {value}"))
        .unwrap_or_default();
    Ok((
        stamp(&error.t)?,
        format!("Boot failed ({}{errno}): {}", error.stage, error.message),
    ))
}

/// Longest record read as written. Longer records are truncated or replaced by a
/// placeholder instead of failing every query for the computer.
const RECORD_LIMIT: u64 = 1024 * 1024;
/// Text kept from a console record over the limit.
const TRUNCATED_TEXT: usize = 64 * 1024;
const TRUNCATED: &str = " … [record over 1 MiB truncated]";

/// Reads at most `RECORD_LIMIT + 1` bytes: more than the limit marks an oversized record.
fn read_record(
    reader: &mut impl BufRead,
    stream: &str,
    bytes: &mut Vec<u8>,
    limit: u64,
) -> std::io::Result<usize> {
    let mut bounded = reader.take(limit.min(RECORD_LIMIT + 1));
    if stream == "boot-error" {
        bounded.read_to_end(bytes)
    } else {
        bounded.read_until(b'\n', bytes)
    }
}
/// Skips the rest of an oversized line within `remaining` bytes. Returns the bytes
/// skipped and whether the line ended with a newline.
fn skip_line(reader: &mut impl BufRead, mut remaining: u64) -> std::io::Result<(u64, bool)> {
    let mut skipped = 0;
    while remaining > 0 {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            break;
        }
        let window = &buffer[..buffer
            .len()
            .min(usize::try_from(remaining).unwrap_or(usize::MAX))];
        if let Some(index) = window.iter().position(|byte| *byte == b'\n') {
            reader.consume(index + 1);
            return Ok((skipped + index as u64 + 1, true));
        }
        let length = window.len();
        reader.consume(length);
        skipped += length as u64;
        remaining -= length as u64;
    }
    Ok((skipped, false))
}
fn truncate_text(text: &mut String) {
    let mut end = TRUNCATED_TEXT.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push_str(TRUNCATED);
}

/// One retained record, as displayed and searched.
struct Decoded {
    in_pem: bool,
    occurred_at: String,
    source: String,
    body: String,
    session: Option<String>,
    /// The file time stands in for a missing or unreadable record time.
    estimated: bool,
    /// The time was parsed from console text the guest wrote.
    guest_time: bool,
    /// The record could not be read as written: a placeholder or truncated text.
    unreadable: bool,
}
/// `raw` holds the record, or only its first bytes when `oversized`. A malformed
/// or oversized record becomes a placeholder instead of an error, so one bad
/// record cannot hide every other record until its segment expires.
fn decode(segment: &Segment, raw: &[u8], oversized: bool) -> Decoded {
    let text = String::from_utf8_lossy(raw);
    let placeholder = |source: &str, body: &str| Decoded {
        in_pem: false,
        occurred_at: segment.modified.clone(),
        source: source.into(),
        body: body.into(),
        session: None,
        estimated: true,
        guest_time: false,
        unreadable: true,
    };
    match segment.stream.as_str() {
        "exec" if oversized => placeholder("system", "[Execution log record over 1 MiB omitted]"),
        "exec" => {
            let Ok(value) = serde_json::from_str::<Value>(&text) else {
                return placeholder("system", "[Unreadable execution log record]");
            };
            let Some(body) = value["d"].as_str() else {
                return placeholder("system", "[Unreadable execution log record]");
            };
            let time = value["t"].as_str().and_then(|time| stamp(time).ok());
            Decoded {
                in_pem: false,
                estimated: time.is_none(),
                occurred_at: time.unwrap_or_else(|| segment.modified.clone()),
                source: value["s"].as_str().unwrap_or("system").to_string(),
                body: if value["e"] == "b64" {
                    "[Binary runtime output]".into()
                } else {
                    body.to_string()
                },
                session: value["id"].as_u64().map(|id| id.to_string()),
                guest_time: false,
                unreadable: false,
            }
        }
        "boot-error" => match (!oversized).then(|| boot_record(&text)) {
            Some(Ok((occurred_at, body))) => Decoded {
                in_pem: false,
                occurred_at,
                source: "runtime".into(),
                body,
                session: None,
                estimated: false,
                guest_time: false,
                unreadable: false,
            },
            _ => placeholder("runtime", "[Unreadable boot failure record]"),
        },
        stream => {
            let clean = runtime_activity::strip_ansi(&text);
            let prefix = clean
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_matches(['[', ']']);
            let parsed = stamp(prefix).ok();
            let mut body = text.trim_end().to_string();
            if oversized {
                truncate_text(&mut body);
            }
            Decoded {
                in_pem: false,
                estimated: parsed.is_none(),
                // Console output is written by the guest, including any time it prints.
                guest_time: parsed.is_some() && stream == "kernel",
                occurred_at: parsed.unwrap_or_else(|| segment.modified.clone()),
                source: stream.to_string(),
                body,
                session: None,
                unreadable: oversized,
            }
        }
    }
}
/// Visits every record of `segment` from `start` up to its recorded length with
/// its offset, identity and whether it ended with a newline. Returns the offset after the last terminated record:
/// an unterminated final record is still being written.
fn scan(
    path: &Path,
    segment: &Segment,
    start: u64,
    redaction: &mut Redaction,
    mut visit: impl FnMut(u64, String, Decoded, bool, &mut Redaction) -> Result<(), String>,
) -> Result<u64, String> {
    let mut file = File::open(path).map_err(|_| "Retained logs could not be opened.")?;
    if file
        .metadata()
        .map_err(|_| "Retained logs could not be read.")?
        .ino()
        != segment.inode
    {
        return Err("Logs rotated during this request. Refresh the search.".into());
    }
    file.seek(SeekFrom::Start(start))
        .map_err(|_| "Retained logs could not be read.")?;
    let mut reader = BufReader::new(file.take(segment.bytes.saturating_sub(start)));
    let mut offset = start;
    let mut consumed = start;
    loop {
        let mut bytes = Vec::new();
        let count = read_record(
            &mut reader,
            &segment.stream,
            &mut bytes,
            segment.bytes.saturating_sub(offset),
        )
        .map_err(|_| "Retained logs could not be read.")? as u64;
        if count == 0 {
            break;
        }
        let oversized = count > RECORD_LIMIT;
        let (length, terminated) = if segment.stream == "boot-error" {
            // One atomically written document fills the file.
            (segment.bytes - offset, true)
        } else if oversized && bytes.last() != Some(&b'\n') {
            let (rest, terminated) = skip_line(&mut reader, segment.bytes - offset - count)
                .map_err(|_| "Retained logs could not be read.")?;
            (count + rest, terminated)
        } else {
            (count, bytes.last() == Some(&b'\n'))
        };
        // An unfinished JSON write is not a record. Plain console chunks can
        // rotate without a newline; their captured bytes remain searchable.
        if segment.stream == "exec" && !terminated {
            break;
        }
        let id = record_id(segment.inode, offset, &bytes);
        let mut decoded = decode(segment, &bytes, oversized);
        redaction.apply(&segment.stream, &mut decoded)?;
        visit(offset, id, decoded, terminated, redaction)?;
        offset += length;
        if terminated {
            consumed = offset;
        }
    }
    Ok(consumed)
}

fn cached_page(
    directory: &Path,
    token: &str,
    binding: &str,
    request: &Query,
    computer_name: &str,
    device_id: &str,
    device_name: &str,
) -> Result<Page, String> {
    let (id, index) = token.split_once(':').ok_or("Invalid log cursor.")?;
    let start: usize = index.parse().map_err(|_| "Invalid log cursor.")?;
    let cached = {
        let mut cache = cache().lock().map_err(|_| "Log query unavailable.")?;
        cache.retain(|_, (seen, _)| seen.elapsed() < Duration::from_secs(1800));
        let (seen, data) = cache
            .get_mut(id)
            .ok_or("Log search expired. Refresh the search.")?;
        if data.binding != binding || start > data.records.len() {
            return Err("The log search changed. Refresh its results.".into());
        }
        *seen = Instant::now();
        data.clone()
    };
    let available = files(directory)?;
    let mut handles = HashMap::new();
    for Indexed { segment, .. } in &cached.files {
        let (path, _) = available
            .iter()
            .find(|(_, s)| {
                s.inode == segment.inode && s.stream == segment.stream && s.bytes >= segment.bytes
            })
            .ok_or("Retained history changed or expired. Refresh the log search.")?;
        let file = File::open(path).map_err(|_| "Retained logs could not be read.")?;
        let metadata = file
            .metadata()
            .map_err(|_| "Retained logs could not be read.")?;
        if metadata.ino() != segment.inode || metadata.len() < segment.bytes {
            return Err("Logs rotated during this request. Refresh the search.".into());
        }
        handles.insert(segment.inode, (BufReader::new(file), segment));
    }
    let mut entries = Vec::new();
    let mut bytes = 0;
    for location in cached
        .records
        .iter()
        .skip(start)
        .take(request.limit.unwrap_or(200).clamp(1, 200))
    {
        let (reader, segment) = handles
            .get_mut(&location.file)
            .ok_or("Retained history expired.")?;
        reader
            .seek(SeekFrom::Start(location.offset))
            .map_err(|_| "Retained log read failed.")?;
        let mut raw_bytes = Vec::new();
        read_record(
            reader,
            &segment.stream,
            &mut raw_bytes,
            segment.bytes.saturating_sub(location.offset),
        )
        .map_err(|_| "Retained log read failed.")?;
        if record_id(location.file, location.offset, &raw_bytes) != location.id {
            return Err("Retained log data changed or expired. Refresh the search.".into());
        }
        let decoded = decode(segment, &raw_bytes, raw_bytes.len() as u64 > RECORD_LIMIT);
        let mut in_pem = location.in_pem;
        let mut entry = Entry {
            id: location.id.clone(),
            line: runtime_activity::log_text_with_pem(&decoded.body, &mut in_pem),
            occurred_at: location.time.clone(),
            computer_id: request.computer_id.clone(),
            computer_name: computer_name.into(),
            device_id: device_id.into(),
            device_name: device_name.into(),
            source: decoded.source,
            session: decoded.session,
            guest_timestamp: decoded.guest_time,
        };
        let mut size = serde_json::to_vec(&entry).map_err(|e| e.to_string())?.len();
        if size as u64 > RECORD_LIMIT {
            // Escaping can grow a record near the limit; show its start instead.
            truncate_text(&mut entry.line);
            size = serde_json::to_vec(&entry).map_err(|e| e.to_string())?.len();
        }
        if bytes + size > 1024 * 1024 {
            break;
        }
        bytes += size;
        entries.push(entry);
    }
    let next = start + entries.len();
    Ok(Page {
        entries,
        next_cursor: (next < cached.records.len()).then(|| format!("{id}:{next}")),
        oldest_available_timestamp: cached.summary.oldest.clone(),
        newest_available_timestamp: cached.summary.newest.clone(),
        total_matches: cached.records.len(),
        timestamp_estimated: cached.summary.estimated,
        unsupported: false,
        unreadable_records: cached.summary.unreadable,
        snapshot: Some(id.to_owned()),
    })
}
fn stamp(value: &str) -> Result<String, String> {
    let time = time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
        .map_err(|_| "Invalid log timestamp.".to_string())?
        .to_offset(time::UtcOffset::UTC);
    Ok(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:09}Z",
        time.year(),
        u8::from(time.month()),
        time.day(),
        time.hour(),
        time.minute(),
        time.second(),
        time.nanosecond()
    ))
}
fn files(directory: &Path) -> Result<Vec<(PathBuf, Segment)>, String> {
    let directory = match fs::read_dir(directory) {
        Ok(directory) => directory,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("Retained logs could not be read.".into()),
    };
    let mut result = Vec::new();
    for file in directory {
        let file = file.map_err(|_| "Retained logs could not be read.")?;
        let name = file.file_name().to_string_lossy().into_owned();
        let stream = if name == "boot-error.json" {
            Some("boot-error")
        } else {
            ["exec", "runtime", "kernel"].into_iter().find(|stream| {
                let base = format!("{stream}.log");
                name == base
                    || name
                        .strip_prefix(&format!("{base}."))
                        .is_some_and(|suffix| {
                            !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit())
                        })
            })
        };
        let Some(stream) = stream else {
            continue;
        };
        let metadata = file
            .path()
            .symlink_metadata()
            .map_err(|_| "Retained log metadata could not be read.")?;
        if !metadata.is_file() {
            return Err("Unexpected retained log file type.".into());
        }
        let modified: time::OffsetDateTime = metadata
            .modified()
            .map_err(|_| "Retained log timestamp unavailable.")?
            .into();
        result.push((
            file.path(),
            Segment {
                inode: metadata.ino(),
                bytes: metadata.len(),
                stream: stream.into(),
                modified: stamp(
                    &modified
                        .format(&time::format_description::well_known::Rfc3339)
                        .map_err(|_| "Invalid log date.")?,
                )?,
            },
        ));
    }
    result.sort_by(|(left, a), (right, b)| {
        let rotation = |path: &Path| {
            path.extension()
                .and_then(|suffix| suffix.to_str())
                .and_then(|suffix| suffix.parse::<u64>().ok())
                .unwrap_or(0)
        };
        a.stream
            .cmp(&b.stream)
            .then_with(|| rotation(right).cmp(&rotation(left)))
    });
    Ok(result)
}
fn record_id(inode: u64, offset: u64, raw: &[u8]) -> String {
    let digest = Sha256::digest(raw);
    let fingerprint = u64::from_be_bytes(digest[..8].try_into().expect("eight hash bytes"));
    format!("{inode}:{offset}:{fingerprint:016x}")
}
fn key(entry: &Entry) -> (String, String) {
    (entry.occurred_at.clone(), entry.id.clone())
}
fn keep(entries: &mut Vec<Entry>, entry: Entry, limit: usize, ascending: bool) {
    let index = entries.partition_point(|old| {
        if ascending {
            key(old) < key(&entry)
        } else {
            key(old) > key(&entry)
        }
    });
    if index < limit {
        entries.insert(index, entry);
        entries.truncate(limit);
    }
}

pub(super) fn is_stopped(status: &str) -> bool {
    status.eq_ignore_ascii_case("stopped") || status.eq_ignore_ascii_case("created")
}
pub(crate) fn query(app: &AppHandle, request: Query) -> Result<Page, BridgeError> {
    let (device_id, device_name) = crate::remote::log_identity()?;
    if let Some(owner) = request
        .device_id
        .as_deref()
        .filter(|id| *id != device_id && *id != "local")
    {
        let outcome = crate::remote::call_remote_typed(
            app,
            owner,
            "runtime.logs",
            serde_json::to_value(&request).map_err(|e| e.to_string())?,
        );
        return remote_page(outcome);
    }
    // A macOS computer keeps its setup log beside Silo's own logs, not in the runtime.
    if let Some((name, directory)) = crate::macos_computers::log_target(app, &request.computer_id) {
        return read(&directory, request, &name, &device_id, &device_name)
            .map_err(BridgeError::from);
    }
    let paths = runtime_paths(app)?;
    query_local(&paths, request, &device_id, &device_name).map_err(BridgeError::from)
}
/// A device running an older Silo rejects `runtime.logs` as an unknown request. That is
/// an expected, structured outcome (an empty page marked unsupported), not a failure.
fn remote_page(outcome: Result<Value, BridgeError>) -> Result<Page, BridgeError> {
    match outcome {
        Ok(value) => serde_json::from_value(value).map_err(|_| {
            "The remote device returned invalid logs. Update Silo on both devices.".into()
        }),
        Err(error) if error.code == ErrorCode::UnsupportedRemoteOperation => Ok(Page {
            entries: Vec::new(),
            next_cursor: None,
            oldest_available_timestamp: None,
            newest_available_timestamp: None,
            total_matches: 0,
            timestamp_estimated: false,
            unsupported: true,
            unreadable_records: false,
            snapshot: None,
        }),
        Err(message) => Err(message),
    }
}
/// Retention truncates and unlinks segments, which is safe only while no runtime
/// writer can start. Hold this computer's gate across the stopped check and the cleanup
/// so a Start cannot be admitted in between. Reading logs observes only: when the
/// Computer is busy, skip the opportunistic cleanup rather than wait.
fn clean_up_if_stopped(
    gate: &operation_gate::OperationGate,
    id: &str,
    name: &str,
    stopped: impl FnOnce() -> bool,
    enforce: impl FnOnce() -> std::io::Result<()>,
) -> Result<(), String> {
    let Ok(_guard) = gate.try_computer_hidden(id, name, "Cleaning up expired logs") else {
        return Ok(());
    };
    if stopped() {
        enforce().map_err(|_| "Expired logs could not be cleaned up.")?;
    }
    Ok(())
}
/// Like `query_local`, for a request from another device: the computer may be a macOS one,
/// whose setup log is kept beside Silo's own logs.
pub(super) fn query_for_remote(
    app: &AppHandle,
    paths: &RuntimePaths,
    request: Query,
    device_id: &str,
    device_name: &str,
) -> Result<Page, String> {
    if let Some((name, directory)) = crate::macos_computers::log_target(app, &request.computer_id) {
        return read(&directory, request, &name, device_id, device_name);
    }
    query_local(paths, request, device_id, device_name)
}
pub(super) fn query_local(
    paths: &RuntimePaths,
    request: Query,
    device_id: &str,
    device_name: &str,
) -> Result<Page, String> {
    let configuration = read_metadata(&paths.metadata).map_err(|e| e.to_string())?;
    let configuration = configuration
        .computers
        .iter()
        .find(|configuration| configuration.id() == request.computer_id)
        .ok_or("This computer no longer exists on this device.")?;
    validate_name(configuration.name()).map_err(|e| e.to_string())?;
    let directory = paths
        .home
        .join("sandboxes")
        .join(configuration.name())
        .join("logs");
    // Follow refreshes run every few seconds; opportunistic cleanup can wait for a search.
    if request.cursor.is_none() && request.follow.is_none() {
        clean_up_if_stopped(
            &OPERATIONS,
            configuration.id(),
            configuration.name(),
            || {
                inspect_computer(&ProcessRunner, paths, configuration.name())
                    .is_ok_and(|computer| is_stopped(&computer.status))
            },
            || crate::log_retention::enforce(&directory),
        )?;
    }
    read(
        &directory,
        request,
        configuration.name(),
        device_id,
        device_name,
    )
}
#[tauri::command]
pub(crate) async fn query_computer_logs(
    app: AppHandle,
    request: Query,
) -> Result<Page, BridgeError> {
    tauri::async_runtime::spawn_blocking(move || query(&app, request))
        .await
        .map_err(|e| e.to_string())?
}
fn read(
    directory: &Path,
    request: Query,
    computer_name: &str,
    device_id: &str,
    device_name: &str,
) -> Result<Page, String> {
    if request.query.as_ref().is_some_and(|q| q.len() > 4096) {
        return Err("Search text is too long.".into());
    }
    if request.source.as_deref().is_some_and(|s| {
        !matches!(
            s,
            "all" | "stdout" | "stderr" | "output" | "system" | "runtime" | "kernel"
        )
    }) {
        return Err("Unknown log source.".into());
    }
    let since = request.since.as_deref().map(stamp).transpose()?;
    let until = request.until.as_deref().map(stamp).transpose()?;
    if since
        .as_ref()
        .zip(until.as_ref())
        .is_some_and(|(s, u)| s > u)
    {
        return Err("The log time range is reversed.".into());
    }
    let binding = serde_json::to_string(&(
        &request.computer_id,
        device_id,
        &request.query,
        &request.source,
        &since,
        &until,
    ))
    .map_err(|e| e.to_string())?;
    let available = files(directory)?;
    if let Some(cursor) = &request.cursor {
        return cached_page(
            directory,
            cursor,
            &binding,
            &request,
            computer_name,
            device_id,
            device_name,
        );
    }
    let filter = Filter {
        since,
        until,
        needle: request.query.as_deref().unwrap_or("").to_lowercase(),
        source: request.source.clone().filter(|source| source != "all"),
    };
    if let Some(around) = &request.around_id {
        return context(
            &available,
            around,
            &request,
            computer_name,
            device_id,
            device_name,
        );
    }
    // Follow continues its previous snapshot; anything unexpected rebuilds it.
    let previous = request.follow.as_deref().and_then(|token| {
        let mut cache = cache().lock().ok()?;
        let (seen, cached) = cache.get_mut(token)?;
        *seen = Instant::now();
        (cached.binding == binding).then(|| cached.clone())
    });
    let followed = match &previous {
        Some(previous) => follow_index(previous, &available, &filter)?,
        None => None,
    };
    let cached = match followed {
        Some(cached) => cached,
        None => {
            let mut summary = Summary::default();
            let mut index = Index::default();
            let mut files = Vec::with_capacity(available.len());
            let mut redaction = Redaction::default();
            for (path, segment) in &available {
                let mut complete = Summary::default();
                let consumed = scan(
                    path,
                    segment,
                    0,
                    &mut redaction,
                    |offset, id, decoded, terminated, redaction| {
                        summary.add(&decoded);
                        if terminated {
                            complete.add(&decoded);
                        }
                        index.add(segment.inode, offset, id, decoded, &filter, redaction)
                    },
                )?;
                files.push(Indexed {
                    segment: segment.clone(),
                    consumed,
                    complete,
                });
            }
            Cached {
                binding: binding.clone(),
                files,
                records: index.sorted(),
                redaction,
                summary,
            }
        }
    };
    let id = store(cached, request.follow.as_deref())?;
    cached_page(
        directory,
        &format!("{id}:0"),
        &binding,
        &request,
        computer_name,
        device_id,
        device_name,
    )
}
/// Search filters of one request.
struct Filter {
    since: Option<String>,
    until: Option<String>,
    needle: String,
    source: Option<String>,
}
impl Filter {
    fn matches(&self, occurred_at: &str, source: &str, line: &str) -> bool {
        self.since
            .as_deref()
            .is_none_or(|since| occurred_at >= since)
            && self
                .until
                .as_deref()
                .is_none_or(|until| occurred_at <= until)
            && self.source.as_deref().is_none_or(|wanted| wanted == source)
            && line.to_lowercase().contains(&self.needle)
    }
}
/// Coverage of every scanned record, matching or not.
#[derive(Clone, Default)]
struct Summary {
    oldest: Option<String>,
    newest: Option<String>,
    estimated: bool,
    unreadable: bool,
}
impl Summary {
    fn add(&mut self, decoded: &Decoded) {
        let time = &decoded.occurred_at;
        if self.oldest.as_ref().is_none_or(|oldest| time < oldest) {
            self.oldest = Some(time.clone());
        }
        if self.newest.as_ref().is_none_or(|newest| time > newest) {
            self.newest = Some(time.clone());
        }
        self.estimated |= decoded.estimated;
        self.unreadable |= decoded.unreadable;
    }
    fn merge(&mut self, other: &Summary) {
        if let Some(oldest) = &other.oldest {
            if self.oldest.as_ref().is_none_or(|current| oldest < current) {
                self.oldest = Some(oldest.clone());
            }
        }
        if let Some(newest) = &other.newest {
            if self.newest.as_ref().is_none_or(|current| newest > current) {
                self.newest = Some(newest.clone());
            }
        }
        self.estimated |= other.estimated;
        self.unreadable |= other.unreadable;
    }
}
/// Offsets of matching records; bodies are re-read per page.
#[derive(Default)]
struct Index {
    records: Vec<Location>,
}
impl Index {
    fn add(
        &mut self,
        file: u64,
        offset: u64,
        id: String,
        decoded: Decoded,
        filter: &Filter,
        redaction: &mut Redaction,
    ) -> Result<(), String> {
        if !filter.matches(&decoded.occurred_at, &decoded.source, &decoded.body) {
            return Ok(());
        }
        redaction.records += location_cost(&decoded.occurred_at, &id);
        if redaction.bytes + redaction.records > redaction.limit {
            return Err(TOO_MANY_MATCHES.into());
        }
        self.records.push(Location {
            file,
            offset,
            in_pem: decoded.in_pem,
            time: decoded.occurred_at,
            id,
        });
        Ok(())
    }
    /// Newest first, the order pages are served in.
    fn sorted(mut self) -> Vec<Location> {
        self.records
            .sort_unstable_by(|a, b| (&b.time, &b.id).cmp(&(&a.time, &a.id)));
        self.records
    }
}
const INDEX_BUDGET: usize = 128 * 1024 * 1024;
/// A following view keeps one snapshot (each refresh replaces its predecessor),
/// so a few searches per view are enough.
const MAX_SNAPSHOTS: usize = 16;
const TOO_MANY_MATCHES: &str =
    "This search has too many matches. Narrow its time range or search text.";
fn location_cost(time: &str, id: &str) -> usize {
    std::mem::size_of::<Location>() + time.len() + id.len()
}
fn cached_cost(cached: &Cached) -> usize {
    cached.redaction.bytes
        + cached
            .records
            .iter()
            .map(|record| location_cost(&record.time, &record.id))
            .sum::<usize>()
}
/// Keep a snapshot for its cursors, replacing the snapshot it follows and evicting
/// the least recently used over budget.
fn store(cached: Cached, replaces: Option<&str>) -> Result<String, String> {
    let cost = cached_cost(&cached);
    if cost > INDEX_BUDGET {
        return Err(TOO_MANY_MATCHES.into());
    }
    let mut cache = cache().lock().map_err(|_| "Log query unavailable.")?;
    if let Some(previous) = replaces {
        cache.remove(previous);
    }
    cache.retain(|_, (seen, _)| seen.elapsed() < Duration::from_secs(1800));
    while cache.len() >= MAX_SNAPSHOTS
        || cache
            .values()
            .map(|(_, cached)| cached_cost(cached))
            .sum::<usize>()
            + cost
            > INDEX_BUDGET
    {
        let Some(oldest) = cache
            .iter()
            .min_by_key(|(_, (seen, _))| *seen)
            .map(|(id, _)| id.clone())
        else {
            break;
        };
        cache.remove(&oldest);
    }
    let id = uuid::Uuid::new_v4().to_string();
    cache.insert(id.clone(), (Instant::now(), std::sync::Arc::new(cached)));
    Ok(id)
}
/// Extends `previous` with what was appended since, reading each surviving file
/// only from its consumed offset and new files in full. Files that expired drop
/// their records. Returns None when a file shrank in place (retention truncation)
/// or a boot failure changed without a new inode: the caller rebuilds instead.
fn follow_index(
    previous: &Cached,
    available: &[(PathBuf, Segment)],
    filter: &Filter,
) -> Result<Option<Cached>, String> {
    // Rebuild when rotation or an unfinished record changes the scanning boundary.
    if previous.files.len() != available.len()
        || previous.files.iter().any(|old| {
            old.consumed != old.segment.bytes
                || !available.iter().any(|(_, segment)| {
                    segment.inode == old.segment.inode && segment.stream == old.segment.stream
                })
        })
    {
        return Ok(None);
    }
    // The clone coexists with every cached snapshot until the refresh replaces its predecessor.
    let retained: usize = cache()
        .lock()
        .map_err(|_| "Log query unavailable.")?
        .values()
        .map(|(_, cached)| cached_cost(cached))
        .sum();
    if retained + previous.redaction.bytes > INDEX_BUDGET {
        return Err(TOO_MANY_MATCHES.into());
    }
    let mut redaction = previous.redaction.clone();
    redaction.records = previous
        .records
        .iter()
        .map(|record| location_cost(&record.time, &record.id))
        .sum();
    let mut summary = Summary::default();
    let mut index = Index::default();
    let mut files = Vec::with_capacity(available.len());
    let mut carried = HashMap::new();
    for (path, segment) in available {
        let old = previous
            .files
            .iter()
            .find(|old| old.segment.inode == segment.inode && old.segment.stream == segment.stream);
        let (start, mut complete) = match old {
            Some(old)
                if segment.bytes < old.segment.bytes
                    || (segment.stream == "boot-error" && segment.bytes != old.segment.bytes) =>
            {
                return Ok(None)
            }
            Some(old) => {
                carried.insert(segment.inode, old.consumed);
                (old.consumed, old.complete.clone())
            }
            None => (0, Summary::default()),
        };
        summary.merge(&complete);
        let consumed = scan(
            path,
            segment,
            start,
            &mut redaction,
            |offset, id, decoded, terminated, redaction| {
                summary.add(&decoded);
                if terminated {
                    complete.add(&decoded);
                }
                index.add(segment.inode, offset, id, decoded, filter, redaction)
            },
        )?;
        files.push(Indexed {
            segment: segment.clone(),
            consumed,
            complete,
        });
    }
    // Both lists are newest first; keep that order while merging.
    let appended = index.sorted();
    let mut records = Vec::with_capacity(previous.records.len() + appended.len());
    let mut kept = previous
        .records
        .iter()
        .filter(|record| {
            carried
                .get(&record.file)
                .is_some_and(|consumed| record.offset < *consumed)
        })
        .peekable();
    let mut appended = appended.into_iter().peekable();
    loop {
        let take_kept = match (kept.peek(), appended.peek()) {
            (Some(old), Some(new)) => (&old.time, &old.id) >= (&new.time, &new.id),
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };
        if take_kept {
            let old = kept.next().expect("peeked");
            records.push(Location {
                file: old.file,
                offset: old.offset,
                in_pem: old.in_pem,
                time: old.time.clone(),
                id: old.id.clone(),
            });
        } else {
            records.push(appended.next().expect("peeked"));
        }
    }
    let cached = Cached {
        binding: previous.binding.clone(),
        files,
        records,
        redaction,
        summary,
    };
    if cached_cost(&cached) > INDEX_BUDGET {
        return Err(TOO_MANY_MATCHES.into());
    }
    Ok(Some(cached))
}
/// Up to 50 records on each side of `around`, across all streams. Filters do not apply.
fn context(
    available: &[(PathBuf, Segment)],
    around: &str,
    request: &Query,
    computer_name: &str,
    device_id: &str,
    device_name: &str,
) -> Result<Page, String> {
    let mut anchor = None;
    let mut redaction = Redaction::default();
    for (path, segment) in available {
        scan(path, segment, 0, &mut redaction, |_, id, decoded, _, _| {
            if anchor.is_none() && id == around {
                anchor = Some((decoded.occurred_at, id));
            }
            Ok(())
        })?;
    }
    let anchor = anchor.ok_or("The selected log record expired. Refresh the log search.")?;
    let mut summary = Summary::default();
    let mut total = 0;
    let mut redaction = Redaction::default();
    let mut older = Vec::new();
    let mut newer = Vec::new();
    for (path, segment) in available {
        scan(path, segment, 0, &mut redaction, |_, id, decoded, _, _| {
            summary.add(&decoded);
            total += 1;
            let entry = Entry {
                id,
                line: decoded.body,
                occurred_at: decoded.occurred_at,
                computer_id: request.computer_id.clone(),
                computer_name: computer_name.into(),
                device_id: device_id.into(),
                device_name: device_name.into(),
                source: decoded.source,
                session: decoded.session,
                guest_timestamp: decoded.guest_time,
            };
            if key(&entry) > anchor {
                keep(&mut newer, entry, 50, true);
            } else {
                keep(&mut older, entry, 51, false);
            }
            Ok(())
        })?;
    }
    let mut entries = older;
    entries.extend(newer);
    entries.sort_by_key(|entry| std::cmp::Reverse(key(entry)));
    if serde_json::to_vec(&entries)
        .map_err(|e| e.to_string())?
        .len()
        > 1024 * 1024
    {
        return Err("This context window is too large. Narrow the time range instead.".into());
    }
    Ok(Page {
        total_matches: total,
        entries,
        next_cursor: None,
        oldest_available_timestamp: summary.oldest,
        newest_available_timestamp: summary.newest,
        timestamp_estimated: summary.estimated,
        unsupported: false,
        unreadable_records: summary.unreadable,
        snapshot: None,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn retention_runs_only_while_holding_the_computer_gate() {
        let gate = super::operation_gate::OperationGate::new();
        // Admission from another thread: this thread's own guard would be a nesting error.
        let start_admitted = || {
            std::thread::scope(|scope| {
                scope
                    .spawn(|| {
                        gate.try_computer("computer-1", "dev", "Starting dev")
                            .is_ok()
                    })
                    .join()
                    .unwrap()
            })
        };
        let busy = gate
            .try_computer("computer-1", "dev", "Starting dev")
            .unwrap();
        super::clean_up_if_stopped(
            &gate,
            "computer-1",
            "dev",
            || panic!("a busy computer is not inspected"),
            || panic!("a busy computer's logs are not cleaned"),
        )
        .unwrap();
        drop(busy);
        let mut cleaned = false;
        super::clean_up_if_stopped(
            &gate,
            "computer-1",
            "dev",
            || !start_admitted(),
            || {
                // A Start cannot be admitted between the stopped check and the cleanup.
                assert!(!start_admitted());
                cleaned = true;
                Ok(())
            },
        )
        .unwrap();
        assert!(cleaned);
        assert!(start_admitted(), "the gate is released after cleanup");
        super::clean_up_if_stopped(
            &gate,
            "computer-1",
            "dev",
            || false,
            || panic!("a running computer's logs are not cleaned"),
        )
        .unwrap();
    }
    #[test]
    fn unsupported_remote_request_becomes_a_structured_outcome() {
        let page = super::remote_page(Err(BridgeError::unsupported())).unwrap();
        assert!(page.unsupported && page.entries.is_empty());
        assert!(
            super::remote_page(Err(BridgeError::new(
                ErrorCode::UnsupportedRemoteOperation,
                "Owner cannot serve logs."
            )))
            .unwrap()
            .unsupported
        );
        assert_eq!(
            super::remote_page(Err("Connection refused.".into()))
                .err()
                .unwrap(),
            BridgeError::from("Connection refused.")
        );
    }
    use super::*;
    #[test]
    fn a_macos_setup_log_is_read_as_the_runtime_source() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("runtime.log"),
            "2026-10-10T12:00:00.500000Z step: Preparing Recovery\n2026-10-10T12:00:01Z personalization script: status 1; stdout: ; stderr: boom\n",
        )
        .unwrap();
        let mut query = request();
        query.source = Some("runtime".into());
        query.query = Some("boom".into());
        let page = read(directory.path(), query, "mac", "pc", "Desktop").unwrap();
        assert_eq!(page.total_matches, 1);
        assert!(page.entries[0].line.contains("status 1"));
        assert!(!page.timestamp_estimated);
    }
    #[test]
    fn boot_failure_is_searchable_with_its_timestamp_context_and_pagination() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("runtime.log"),
            "2026-09-22T09:19:32.455Z entering computer\n",
        )
        .unwrap();
        fs::write(
            directory.path().join("boot-error.json"),
            serde_json::to_string_pretty(&json!({
                "t": "2026-09-22T09:19:32.467Z", "stage": "build_computer", "errno": null,
                "message": "libkrunfw could not load: different Team IDs\nTOKEN=private-value",
            }))
            .unwrap(),
        )
        .unwrap();
        let mut query = request();
        query.query = Some("different Team IDs".into());
        query.source = Some("runtime".into());
        query.since = Some("2026-09-22T09:19:32Z".into());
        query.until = Some("2026-09-22T09:19:33Z".into());
        let page = read(directory.path(), query, "dev", "pc", "Desktop").unwrap();
        assert_eq!(page.total_matches, 1);
        assert_eq!(page.entries[0].source, "runtime");
        assert_eq!(
            page.entries[0].occurred_at,
            "2026-09-22T09:19:32.467000000Z"
        );
        assert!(page.entries[0].line.contains("build_computer"));
        assert!(!page.entries[0].line.contains("private-value"));
        assert!(!page.timestamp_estimated);
        let mut context = request();
        context.around_id = Some(page.entries[0].id.clone());
        assert_eq!(
            read(directory.path(), context, "dev", "pc", "Desktop")
                .unwrap()
                .entries
                .len(),
            2
        );
        let mut query = request();
        query.limit = Some(1);
        let first = read(directory.path(), query.clone(), "dev", "pc", "Desktop").unwrap();
        assert_eq!(first.entries[0].id, page.entries[0].id);
        query.cursor = first.next_cursor;
        let second = read(directory.path(), query, "dev", "pc", "Desktop").unwrap();
        assert_eq!(second.entries.len(), 1);
        assert!(second.entries[0].line.contains("entering computer"));
    }

    #[test]
    fn replacing_a_boot_failure_expires_the_old_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("runtime.log"),
            "2026-09-22T09:19:30Z entering computer\n",
        )
        .unwrap();
        let boot = |message| {
            serde_json::to_vec(
                &json!({ "t": "2026-09-22T09:19:32Z", "stage": "build_computer", "message": message }),
            )
            .unwrap()
        };
        fs::write(
            directory.path().join("boot-error.json"),
            boot("first attempt"),
        )
        .unwrap();
        let mut query = request();
        query.limit = Some(1);
        let first = read(directory.path(), query.clone(), "dev", "pc", "Desktop").unwrap();
        fs::write(
            directory.path().join("next-boot-error.json"),
            boot("next attempt"),
        )
        .unwrap();
        fs::rename(
            directory.path().join("next-boot-error.json"),
            directory.path().join("boot-error.json"),
        )
        .unwrap();
        query.cursor = first.next_cursor;
        assert!(read(directory.path(), query, "dev", "pc", "Desktop")
            .err()
            .unwrap()
            .contains("expired"));
    }

    fn request() -> Query {
        Query {
            computer_id: "computer-1".into(),
            ..Query::default()
        }
    }
    fn line(index: usize, body: &str) -> String {
        format!("{{\"t\":\"2026-09-18T12:00:00.{index:09}Z\",\"s\":\"stderr\",\"d\":\"{body}\",\"id\":42}}\n")
    }
    #[test]
    fn credential_urls_are_hidden_in_pages_context_and_export() {
        let directory = tempfile::tempdir().unwrap();
        let mut records = line(0, "ordinary connection failure");
        for (index, body) in [
            "fetch failed: https://alice:synthetic-url-password@example.test/repo",
            "git clone 'https://synthetic-url-token@example.test/repo'",
            "connect(postgresql://alice:synthetic-db-password@localhost/db)",
            "remote=https://alice:synthetic%2Dencoded%2Dpassword@example.test/repo",
            "curl --user alice:synthetic-password https://example.test",
            "curl -u alice:synthetic-password https://example.test",
            "login --password synthetic-password",
            "client --api-key synthetic-key",
            "client --client-secret synthetic-secret",
            "client --access_token synthetic-token",
            "download https://example.test/blob?sv=2026-02-06&sp=r&sig=synthetic-signature",
            "fetch https://example.test/?%74oken=synthetic-query-token",
            "fetch https://example.test/?api%5Fkey=synthetic-query-key",
        ]
        .iter()
        .enumerate()
        {
            records.push_str(&line(index + 1, body));
        }
        fs::write(directory.path().join("exec.log"), records).unwrap();
        let mut query = request();
        query.limit = Some(1);
        let first = read(directory.path(), query.clone(), "dev", "pc", "Desktop").unwrap();
        query.cursor = first.next_cursor.clone();
        let page = read(directory.path(), query, "dev", "pc", "Desktop").unwrap();
        let mut context = request();
        context.around_id = Some(page.entries[0].id.clone());
        let context = read(directory.path(), context, "dev", "pc", "Desktop").unwrap();
        let mut exported = Vec::new();
        crate::log_export::write_requests(
            &mut exported,
            vec![request()],
            |query| read(directory.path(), query, "dev", "pc", "Desktop"),
            || false,
        )
        .unwrap();
        for text in [
            serde_json::to_string(&first).unwrap(),
            serde_json::to_string(&page).unwrap(),
            serde_json::to_string(&context).unwrap(),
            String::from_utf8(exported).unwrap(),
        ] {
            assert!(!text.contains("synthetic"), "Credentials escaped: {text}");
        }
        assert!(context
            .entries
            .iter()
            .any(|entry| entry.line == "ordinary connection failure"));
    }
    fn disposable_pem() -> Vec<String> {
        let output = Command::new("openssl")
            .args([
                "genpkey",
                "-algorithm",
                "RSA",
                "-pkeyopt",
                "rsa_keygen_bits:1024",
            ])
            .output()
            .expect("generate a disposable test key");
        assert!(output.status.success());
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn assert_no_key(text: &str, pem: &[String]) {
        assert!(
            pem[1..pem.len() - 1]
                .iter()
                .all(|body| !text.contains(body)),
            "disposable private-key body survived redaction"
        );
    }

    #[test]
    fn pem_records_are_hidden_in_search_pages_context_follow_and_export() {
        let directory = tempfile::tempdir().unwrap();
        let pem = disposable_pem();
        let records = |lines: &[String], start: usize| {
            lines
                .iter()
                .enumerate()
                .map(|(i, body)| line(start + i, body))
                .collect::<String>()
        };
        fs::write(directory.path().join("exec.log"), records(&pem[..2], 0)).unwrap();
        let first = read(directory.path(), request(), "dev", "pc", "Desktop").unwrap();
        assert_no_key(&serde_json::to_string(&first).unwrap(), &pem);
        // A different command remains readable while this session's PEM is open.
        let mut appended = line(2, "ordinary\\nmultiline output").replace("\"id\":42", "\"id\":99");
        appended.push_str(&records(&pem[2..], 3));
        appended.push_str(&line(99, "after key"));
        fs::OpenOptions::new()
            .append(true)
            .open(directory.path().join("exec.log"))
            .unwrap()
            .write_all(appended.as_bytes())
            .unwrap();
        let mut follow = request();
        follow.follow = first.snapshot;
        let followed = read(directory.path(), follow, "dev", "pc", "Desktop").unwrap();
        assert_no_key(&serde_json::to_string(&followed).unwrap(), &pem);
        assert!(followed
            .entries
            .iter()
            .any(|e| e.line == "ordinary\nmultiline output"));
        assert!(followed.entries.iter().any(|e| e.line == "after key"));

        for source in ["exec", "kernel"] {
            if source == "kernel" {
                fs::write(directory.path().join("kernel.log"), pem.join("\n") + "\n").unwrap();
            }
            let mut query = request();
            query.limit = Some(1);
            let mut pages = Vec::new();
            loop {
                let page = read(directory.path(), query.clone(), "dev", "pc", "Desktop").unwrap();
                assert_no_key(&serde_json::to_string(&page).unwrap(), &pem);
                pages.extend(page.entries);
                query.cursor = page.next_cursor;
                if query.cursor.is_none() {
                    break;
                }
            }
            let mut context = request();
            context.around_id = Some(pages[pages.len() / 2].id.clone());
            let context = read(directory.path(), context, "dev", "pc", "Desktop").unwrap();
            assert_no_key(&serde_json::to_string(&context).unwrap(), &pem);
            let mut search = request();
            search.query = Some(pem[1].clone());
            assert_eq!(
                read(directory.path(), search, "dev", "pc", "Desktop")
                    .unwrap()
                    .total_matches,
                0
            );
            let mut export = Vec::new();
            let mut query = request();
            query.limit = Some(1);
            crate::log_export::write_requests(
                &mut export,
                vec![query],
                |query| read(directory.path(), query, "dev", "pc", "Desktop"),
                || false,
            )
            .unwrap();
            assert_no_key(&String::from_utf8(export).unwrap(), &pem);
        }
    }

    #[test]
    fn pem_redaction_spans_rotation_and_unterminated_blocks() {
        let directory = tempfile::tempdir().unwrap();
        let pem = disposable_pem();
        for stream in ["kernel", "exec"] {
            let records = |lines: &[String], start| {
                if stream == "kernel" {
                    lines.join("\n") + "\n"
                } else {
                    lines
                        .iter()
                        .enumerate()
                        .map(|(i, body)| line(start + i, body))
                        .collect::<String>()
                }
            };
            fs::write(
                directory.path().join(format!("{stream}.log.12")),
                records(&pem[..2], 0),
            )
            .unwrap();
            fs::write(
                directory.path().join(format!("{stream}.log.2")),
                records(&pem[2..4], 2),
            )
            .unwrap();
            fs::write(
                directory.path().join(format!("{stream}.log")),
                records(&pem[4..pem.len() - 1], 4),
            )
            .unwrap();
        }
        let first = read(directory.path(), request(), "dev", "pc", "Desktop").unwrap();
        assert_no_key(&serde_json::to_string(&first).unwrap(), &pem);
        fs::rename(
            directory.path().join("exec.log"),
            directory.path().join("exec.log.1"),
        )
        .unwrap();
        fs::write(
            directory.path().join("exec.log"),
            line(100, &pem[1]) + &line(101, pem.last().unwrap()) + &line(102, "after rotation"),
        )
        .unwrap();
        let mut follow = request();
        follow.follow = first.snapshot;
        let page = read(directory.path(), follow, "dev", "pc", "Desktop").unwrap();
        assert_no_key(&serde_json::to_string(&page).unwrap(), &pem);
        assert!(page.entries.iter().any(|e| e.line == "after rotation"));
    }

    #[test]
    fn search_finds_old_error_across_more_than_100000_rotated_records() {
        let directory = tempfile::tempdir().unwrap();
        let mut old = File::create(directory.path().join("exec.log.12")).unwrap();
        for index in 0..100_001 {
            old.write_all(
                line(
                    index,
                    if index == 17 {
                        "historic failure"
                    } else {
                        "ordinary output"
                    },
                )
                .as_bytes(),
            )
            .unwrap();
        }
        fs::write(directory.path().join("exec.log"), line(100_002, "latest")).unwrap();
        let mut query = request();
        query.query = Some("historic failure".into());
        let page = read(directory.path(), query, "dev", "device", "Desktop").unwrap();
        assert_eq!(page.total_matches, 1);
        assert_eq!(page.entries[0].line, "historic failure");
        assert_eq!(page.entries[0].session.as_deref(), Some("42"));
        assert_eq!(page.entries[0].device_name, "Desktop");
        assert!(page.next_cursor.is_none());
        let started = Instant::now();
        let mut query = request();
        let mut count = 0;
        loop {
            let page = read(directory.path(), query.clone(), "dev", "device", "Desktop").unwrap();
            count += page.entries.len();
            query.cursor = page.next_cursor;
            if query.cursor.is_none() {
                break;
            }
        }
        assert_eq!(count, 100_002);
        eprintln!("100,002-record paginated export: {:?}", started.elapsed());
    }
    #[test]
    fn pagination_survives_append_and_rename_without_gaps_or_duplicates() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("exec.log"),
            (0..550).map(|i| line(i, "record")).collect::<String>(),
        )
        .unwrap();
        let first = read(directory.path(), request(), "dev", "pc", "Desktop").unwrap();
        assert_eq!(first.entries.len(), 200);
        fs::rename(
            directory.path().join("exec.log"),
            directory.path().join("exec.log.1"),
        )
        .unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(directory.path().join("exec.log.1"))
            .unwrap()
            .write_all(line(551, "appended").as_bytes())
            .unwrap();
        fs::write(directory.path().join("exec.log"), line(552, "new file")).unwrap();
        let mut ids: HashSet<_> = first.entries.iter().map(|e| e.id.clone()).collect();
        let mut cursor = first.next_cursor;
        while cursor.is_some() {
            let mut query = request();
            query.cursor = cursor;
            let page = read(directory.path(), query, "dev", "pc", "Desktop").unwrap();
            assert_eq!(page.total_matches, 550);
            for entry in page.entries {
                assert!(ids.insert(entry.id));
                assert_eq!(entry.line, "record");
            }
            cursor = page.next_cursor;
        }
        assert_eq!(ids.len(), 550);
    }
    #[test]
    fn context_filters_redaction_and_expired_cursor_are_explicit() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("exec.log"),
            (0..300)
                .map(|i| {
                    line(
                        i,
                        if i == 150 {
                            "old error"
                        } else {
                            "Bearer private-token"
                        },
                    )
                })
                .collect::<String>(),
        )
        .unwrap();
        let mut query = request();
        query.query = Some("old error".into());
        let found = read(directory.path(), query, "dev", "pc", "Desktop").unwrap();
        let mut query = request();
        query.around_id = Some(found.entries[0].id.clone());
        let context = read(directory.path(), query, "dev", "pc", "Desktop").unwrap();
        assert_eq!(context.entries.len(), 101);
        assert_eq!(context.entries[50].line, "old error");
        assert!(context
            .entries
            .iter()
            .all(|e| !e.line.contains("private-token")));
        let first = read(directory.path(), request(), "dev", "pc", "Desktop").unwrap();
        fs::remove_file(directory.path().join("exec.log")).unwrap();
        let mut query = request();
        query.cursor = first.next_cursor;
        assert!(read(directory.path(), query, "dev", "pc", "Desktop")
            .err()
            .unwrap()
            .contains("expired"));
    }
    #[test]
    fn replaced_content_with_same_inode_and_length_invalidates_cursor() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("exec.log");
        fs::write(
            &path,
            (0..300).map(|i| line(i, "original")).collect::<String>(),
        )
        .unwrap();
        let first = read(directory.path(), request(), "dev", "pc", "Desktop").unwrap();
        // Retention truncates the current inode; a writer can regrow it before the next page.
        fs::write(
            &path,
            (0..300).map(|i| line(i, "replaced")).collect::<String>(),
        )
        .unwrap();
        let mut query = request();
        query.cursor = first.next_cursor;
        assert!(read(directory.path(), query, "dev", "pc", "Desktop")
            .err()
            .unwrap()
            .contains("changed or expired"));
    }

    #[test]
    fn runtime_state_casing_and_non_utf8_console_output_are_supported() {
        for status in ["Stopped", "Created", "stopped", "created"] {
            assert!(is_stopped(status));
        }
        for status in ["Running", "Starting", "Failed", "unknown"] {
            assert!(!is_stopped(status));
        }
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("kernel.log"),
            b"console \xff output\n",
        )
        .unwrap();
        let page = read(directory.path(), request(), "dev", "pc", "Desktop").unwrap();
        assert_eq!(page.entries.len(), 1);
        assert!(page.entries[0].line.contains("console"));
    }
    #[test]
    fn search_and_export_preserve_text_beyond_old_display_limit() {
        let directory = tempfile::tempdir().unwrap();
        let body = format!("{} historical failure", "x".repeat(8000));
        fs::write(directory.path().join("exec.log"), line(1, &body)).unwrap();
        let mut query = request();
        query.query = Some("historical failure".into());
        let page = read(directory.path(), query, "dev", "pc", "Desktop").unwrap();
        assert_eq!(page.entries[0].line, body);
    }

    #[test]
    fn plain_text_rotation_fragments_remain_searchable_and_exportable() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("kernel.log.1"),
            b"historical kernel failure",
        )
        .unwrap();
        fs::write(
            directory.path().join("runtime.log"),
            b"current runtime failure",
        )
        .unwrap();
        fs::write(directory.path().join("exec.log"), b"{\"t\":").unwrap();
        let mut query = request();
        query.query = Some("failure".into());
        let page = read(directory.path(), query, "dev", "pc", "Desktop").unwrap();
        assert_eq!(page.entries.len(), 2);
        assert!(page
            .entries
            .iter()
            .any(|entry| entry.line == "historical kernel failure"));
        assert!(page
            .entries
            .iter()
            .any(|entry| entry.line == "current runtime failure"));
    }

    #[test]
    fn ansi_diagnostics_preserve_dates_search_and_redaction() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("runtime.log"), b"\x1b[32m2026-09-18T08:00:00Z\x1b[0m red \x1b[31mfailure\x1b[0m\nAuth\x1b[31morization: private-value\n").unwrap();
        let page = read(directory.path(), request(), "dev", "pc", "Desktop").unwrap();
        assert!(page
            .entries
            .iter()
            .any(|entry| entry.line == "[Sensitive runtime output hidden]"));
        assert!(page
            .entries
            .iter()
            .all(|entry| !entry.line.contains("[31m") && !entry.line.contains("private-value")));
        let mut query = request();
        query.query = Some("red failure".into());
        let found = read(directory.path(), query, "dev", "pc", "Desktop").unwrap();
        assert_eq!(found.entries.len(), 1);
        assert_eq!(
            found.entries[0].occurred_at,
            "2026-09-18T08:00:00.000000000Z"
        );
    }

    fn all_pages(directory: &Path, mut query: Query) -> (Vec<Entry>, Page) {
        let first = read(directory, query.clone(), "dev", "pc", "Desktop").unwrap();
        let mut entries = first.entries.clone();
        query.cursor = first.next_cursor.clone();
        while query.cursor.is_some() {
            let page = read(directory, query.clone(), "dev", "pc", "Desktop").unwrap();
            entries.extend(page.entries);
            query.cursor = page.next_cursor;
        }
        (entries, first)
    }
    fn summary_of(entries: &[Entry]) -> Vec<(String, String, String)> {
        entries
            .iter()
            .map(|entry| {
                (
                    entry.id.clone(),
                    entry.occurred_at.clone(),
                    entry.line.clone(),
                )
            })
            .collect()
    }
    fn append(path: &Path, text: &str) {
        std::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }
    #[test]
    fn follow_reads_appended_records_and_matches_a_full_refresh() {
        let directory = tempfile::tempdir().unwrap();
        let exec = directory.path().join("exec.log");
        fs::write(
            &exec,
            (0..300).map(|i| line(i, "record")).collect::<String>(),
        )
        .unwrap();
        let kernel = directory.path().join("kernel.log");
        fs::write(
            &kernel,
            "2026-09-18T12:00:00.000000100Z kernel start\n2026-09-18T12:00:00.000000200Z partial",
        )
        .unwrap();
        let first = read(directory.path(), request(), "dev", "pc", "Desktop").unwrap();
        let snapshot = first
            .snapshot
            .clone()
            .expect("a first page names its snapshot");
        // Appends, a completed console line, a rotation and a new segment.
        append(&exec, &line(301, "appended"));
        append(&kernel, " line completed\n");
        fs::rename(&exec, directory.path().join("exec.log.1")).unwrap();
        fs::write(&exec, line(302, "new segment")).unwrap();
        let mut follow = request();
        follow.follow = Some(snapshot.clone());
        let (followed, followed_first) = all_pages(directory.path(), follow);
        let (full, full_first) = all_pages(directory.path(), request());
        assert_eq!(summary_of(&followed), summary_of(&full));
        // 301 execution records in the rotated file, one in the new file, two console lines.
        assert_eq!(followed_first.total_matches, 304);
        assert_eq!(
            (
                followed_first.oldest_available_timestamp,
                followed_first.newest_available_timestamp
            ),
            (
                full_first.oldest_available_timestamp,
                full_first.newest_available_timestamp
            )
        );
        assert!(followed
            .iter()
            .any(|entry| entry.line.ends_with("partial line completed")));
        assert!(!followed.iter().any(|entry| entry.line.ends_with("partial")));
        // A follow replaces its predecessor instead of accumulating snapshots.
        let mut stale = request();
        stale.cursor = Some(format!("{snapshot}:0"));
        assert!(read(directory.path(), stale, "dev", "pc", "Desktop")
            .err()
            .unwrap()
            .contains("expired"));
    }
    fn unterminated_sessions(count: usize) -> String {
        (0..count)
            .map(|id| line(id, "-----BEGIN PRIVATE KEY-----"))
            .collect()
    }

    #[test]
    fn open_pem_sessions_count_toward_snapshot_cost_and_the_scan_limit() {
        let directory = tempfile::tempdir().unwrap();
        let mut text = unterminated_sessions(40);
        text.push_str(&line(40, "secret-body-line"));
        fs::write(directory.path().join("exec.log"), &text).unwrap();
        let mut query = request();
        query.query = Some("absent-text".into());
        let page = read(directory.path(), query, "dev", "pc", "Desktop").unwrap();
        assert_eq!(page.total_matches, 0);
        let cost = cached_cost(
            &cache()
                .lock()
                .unwrap()
                .get(page.snapshot.as_deref().unwrap())
                .unwrap()
                .1,
        );
        assert!(cost > 0);

        let (path, segment) = files(directory.path()).unwrap().remove(0);
        let mut limited = Redaction {
            limit: cost / 2,
            ..Default::default()
        };
        let err = scan(&path, &segment, 0, &mut limited, |_, _, _, _, _| Ok(())).unwrap_err();
        assert_eq!(err, TOO_MANY_MATCHES);
        assert!(limited.bytes <= limited.limit);

        // Within the limit, later lines of an open block stay hidden.
        let mut roomy = Redaction::default();
        let mut bodies = Vec::new();
        scan(&path, &segment, 0, &mut roomy, |_, _, decoded, _, _| {
            bodies.push(decoded.body);
            Ok(())
        })
        .unwrap();
        assert!(bodies.iter().all(|body| !body.contains("secret-body-line")));
        assert_eq!(roomy.bytes, cost);
    }

    fn open_session_with_records(extra: usize) -> String {
        let mut text = line(0, "-----BEGIN PRIVATE KEY-----");
        for index in 1..=extra {
            text.push_str(&line(index, "secret-body-line"));
        }
        text
    }
    fn match_everything() -> Filter {
        Filter {
            since: None,
            until: None,
            needle: String::new(),
            source: None,
        }
    }

    #[test]
    fn scan_limits_records_and_redaction_state_together() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("exec.log"),
            open_session_with_records(3),
        )
        .unwrap();
        let (path, segment) = files(directory.path()).unwrap().remove(0);
        let filter = match_everything();

        let mut roomy = Redaction::default();
        let mut index = Index::default();
        scan(
            &path,
            &segment,
            0,
            &mut roomy,
            |offset, id, decoded, _, redaction| {
                index.add(segment.inode, offset, id, decoded, &filter, redaction)
            },
        )
        .unwrap();
        assert_eq!(index.records.len(), 4);
        assert!(roomy.bytes > 0 && roomy.records > 0);

        // Each part fits the limit alone; their sum does not.
        let mut shared = Redaction {
            limit: roomy.bytes + roomy.records - 1,
            ..Default::default()
        };
        assert!(roomy.bytes < shared.limit && roomy.records < shared.limit);
        let mut index = Index::default();
        let err = scan(
            &path,
            &segment,
            0,
            &mut shared,
            |offset, id, decoded, _, redaction| {
                index.add(segment.inode, offset, id, decoded, &filter, redaction)
            },
        )
        .unwrap_err();
        assert_eq!(err, TOO_MANY_MATCHES);
        assert_eq!(index.records.len(), 3);
    }

    #[test]
    fn a_new_pem_session_is_refused_when_records_leave_no_room() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("exec.log"),
            line(0, "-----BEGIN PRIVATE KEY-----"),
        )
        .unwrap();
        let (path, segment) = files(directory.path()).unwrap().remove(0);
        let mut redaction = Redaction {
            limit: Redaction::session_cost(&("exec".to_string(), Some("42".to_string()))),
            records: 1,
            ..Default::default()
        };
        let err = scan(&path, &segment, 0, &mut redaction, |_, _, _, _, _| Ok(())).unwrap_err();
        assert_eq!(err, TOO_MANY_MATCHES);
        assert!(redaction.sessions.is_empty());
    }

    #[test]
    fn follow_limits_carried_records_and_redaction_state_together() {
        let directory = tempfile::tempdir().unwrap();
        let exec = directory.path().join("exec.log");
        fs::write(&exec, open_session_with_records(2)).unwrap();
        let (path, segment) = files(directory.path()).unwrap().remove(0);
        let filter = match_everything();
        let mut redaction = Redaction::default();
        let mut index = Index::default();
        let consumed = scan(
            &path,
            &segment,
            0,
            &mut redaction,
            |offset, id, decoded, _, redaction| {
                index.add(segment.inode, offset, id, decoded, &filter, redaction)
            },
        )
        .unwrap();
        let mut previous = Cached {
            binding: String::new(),
            files: vec![Indexed {
                segment,
                consumed,
                complete: Summary::default(),
            }],
            records: index.sorted(),
            redaction,
            summary: Summary::default(),
        };
        previous.redaction.limit = cached_cost(&previous);

        let available = files(directory.path()).unwrap();
        let unchanged = follow_index(&previous, &available, &match_everything()).unwrap();
        assert!(unchanged.is_some());

        let mut appended = open_session_with_records(2);
        appended.push_str(&line(3, "more"));
        fs::write(&exec, appended).unwrap();
        let available = files(directory.path()).unwrap();
        let result = follow_index(&previous, &available, &match_everything());
        assert_eq!(result.err().as_deref(), Some(TOO_MANY_MATCHES));
    }

    #[test]
    fn follow_refuses_to_clone_redaction_state_beyond_the_shared_budget() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("exec.log"), unterminated_sessions(5)).unwrap();
        let (path, segment) = files(directory.path()).unwrap().remove(0);
        let filter = match_everything();
        let mut redaction = Redaction::default();
        let mut index = Index::default();
        let consumed = scan(
            &path,
            &segment,
            0,
            &mut redaction,
            |offset, id, decoded, _, redaction| {
                index.add(segment.inode, offset, id, decoded, &filter, redaction)
            },
        )
        .unwrap();
        assert!(redaction.bytes > 0);
        // A clone of this state alone exceeds the shared budget, so the result does not
        // depend on what parallel tests hold in the process-wide cache.
        redaction.bytes = INDEX_BUDGET + 1;
        let previous = Cached {
            binding: String::new(),
            files: vec![Indexed {
                segment,
                consumed,
                complete: Summary::default(),
            }],
            records: index.sorted(),
            redaction,
            summary: Summary::default(),
        };
        let available = files(directory.path()).unwrap();
        let result = follow_index(&previous, &available, &filter);
        assert_eq!(result.err().as_deref(), Some(TOO_MANY_MATCHES));
    }

    #[test]
    fn follow_does_not_reread_records_it_already_indexed() {
        let directory = tempfile::tempdir().unwrap();
        let exec = directory.path().join("exec.log");
        fs::write(
            &exec,
            (0..100).map(|i| line(i, "record")).collect::<String>(),
        )
        .unwrap();
        let first = read(directory.path(), request(), "dev", "pc", "Desktop").unwrap();
        let previous = cache().lock().unwrap()[first.snapshot.as_deref().unwrap()]
            .1
            .clone();
        // Overwrite indexed bytes in place: a rescan would find unreadable records.
        let mut bytes = fs::read(&exec).unwrap();
        bytes[..10].copy_from_slice(b"##########");
        let mut file = std::fs::OpenOptions::new().write(true).open(&exec).unwrap();
        file.write_all(&bytes).unwrap();
        file.write_all(line(100, "appended").as_bytes()).unwrap();
        drop(file);
        let filter = Filter {
            since: None,
            until: None,
            needle: String::new(),
            source: None,
        };
        let next = follow_index(&previous, &files(directory.path()).unwrap(), &filter)
            .unwrap()
            .unwrap();
        assert_eq!(next.records.len(), 101);
        assert!(!next.summary.unreadable, "indexed bytes were read again");
        assert_eq!(next.files[0].consumed, fs::metadata(&exec).unwrap().len());
    }
    #[test]
    fn follow_rebuilds_after_truncation_or_an_unknown_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let exec = directory.path().join("exec.log");
        fs::write(&exec, (0..50).map(|i| line(i, "old")).collect::<String>()).unwrap();
        let first = read(directory.path(), request(), "dev", "pc", "Desktop").unwrap();
        // Retention truncates the current inode in place; the writer starts again.
        fs::OpenOptions::new()
            .write(true)
            .open(&exec)
            .unwrap()
            .set_len(0)
            .unwrap();
        append(&exec, &line(60, "after truncation"));
        for token in [first.snapshot.clone().unwrap(), "unknown-snapshot".into()] {
            let mut follow = request();
            follow.follow = Some(token);
            let page = read(directory.path(), follow, "dev", "pc", "Desktop").unwrap();
            assert_eq!(page.total_matches, 1);
            assert_eq!(page.entries[0].line, "after truncation");
        }
    }
    #[test]
    fn malformed_exec_records_become_placeholders_instead_of_failing_every_query() {
        let directory = tempfile::tempdir().unwrap();
        let exec = [
            line(1, "before"),
            "not json from a broken writer\n".into(),
            "{\"s\":\"stdout\",\"d\":\"no timestamp\"}\n".into(),
            line(4, "after"),
        ]
        .concat();
        fs::write(directory.path().join("exec.log"), exec).unwrap();
        fs::write(directory.path().join("boot-error.json"), "{ truncated").unwrap();
        let mut query = request();
        query.limit = Some(2);
        let (entries, first) = all_pages(directory.path(), query);
        assert_eq!(first.total_matches, 5);
        assert!(first.unreadable_records);
        assert!(first.timestamp_estimated);
        let lines: Vec<_> = entries.iter().map(|entry| entry.line.as_str()).collect();
        for expected in [
            "before",
            "after",
            "no timestamp",
            "[Unreadable execution log record]",
            "[Unreadable boot failure record]",
        ] {
            assert!(lines.contains(&expected), "{expected}: {lines:?}");
        }
    }
    #[test]
    fn malformed_execution_body_is_flagged_even_when_its_json_is_valid() {
        for record in [
            "null",
            "[]",
            "{}",
            "\"unexpected text\"",
            r#"{"t":"2026-09-18T12:00:00Z","s":"stderr","d":42}"#,
        ] {
            let directory = tempfile::tempdir().unwrap();
            fs::write(
                directory.path().join("exec.log"),
                format!("{record}\n{}", line(1, "after")),
            )
            .unwrap();
            let page = read(directory.path(), request(), "dev", "pc", "Desktop").unwrap();
            assert!(page.unreadable_records, "{record}");
            assert_eq!(page.total_matches, 2);
            assert!(page
                .entries
                .iter()
                .any(|entry| entry.line == "[Unreadable execution log record]"));
            assert!(page.entries.iter().any(|entry| entry.line == "after"));
        }
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("exec.log"),
            format!(
                "{}{}\n",
                line(0, ""),
                r#"{"t":"2026-09-18T12:00:00Z","s":"stdout","e":"b64","d":"AA=="}"#,
            ),
        )
        .unwrap();
        let page = read(directory.path(), request(), "dev", "pc", "Desktop").unwrap();
        assert!(!page.unreadable_records);
        assert!(page.entries.iter().any(|entry| entry.line.is_empty()));
        assert!(page
            .entries
            .iter()
            .any(|entry| entry.line == "[Binary runtime output]"));
    }
    #[test]
    fn oversized_record_ending_at_the_read_boundary_preserves_the_next_record() {
        for stream in ["kernel", "exec"] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join(format!("{stream}.log"));
            fs::write(&path, "").unwrap();
            let initial = read(directory.path(), request(), "dev", "pc", "Desktop").unwrap();
            let oversized = if stream == "exec" {
                line(
                    0,
                    &"x".repeat(RECORD_LIMIT as usize + 1 - line(0, "").len()),
                )
            } else {
                format!("{}\n", "x".repeat(RECORD_LIMIT as usize))
            };
            assert_eq!(oversized.len(), RECORD_LIMIT as usize + 1);
            append(&path, &oversized);
            for (index, body) in [(1, "first sentinel"), (2, "second sentinel")] {
                append(
                    &path,
                    &if stream == "exec" {
                        line(index, body)
                    } else {
                        format!("2026-09-18T12:00:00.{index:09}Z {body}\n")
                    },
                );
            }
            let mut follow = request();
            follow.follow = initial.snapshot;
            follow.limit = Some(1);
            let (entries, first) = all_pages(directory.path(), follow);
            assert_eq!(first.total_matches, 3, "{stream}");
            for sentinel in ["first sentinel", "second sentinel"] {
                assert!(entries.iter().any(|entry| entry.line.ends_with(sentinel)));
            }
            assert!(first.unreadable_records);
            let mut search = request();
            search.query = Some("first sentinel".into());
            let found = read(directory.path(), search, "dev", "pc", "Desktop").unwrap();
            assert_eq!(found.total_matches, 1, "{stream}");
            let mut context = request();
            context.around_id = Some(found.entries[0].id.clone());
            let around = read(directory.path(), context, "dev", "pc", "Desktop").unwrap();
            assert_eq!(around.entries.len(), 3, "{stream}");
        }
    }
    #[test]
    fn an_oversized_console_record_is_truncated_and_later_records_stay_readable() {
        let directory = tempfile::tempdir().unwrap();
        // A guest can write megabytes to /dev/console without a newline.
        let mut kernel = "flood ".repeat(400_000).into_bytes();
        kernel.extend_from_slice(b"\n2026-09-18T08:00:01Z after the flood\n");
        kernel.extend("unterminated ".repeat(100_000).as_bytes());
        fs::write(directory.path().join("kernel.log"), kernel).unwrap();
        let mut exec = vec![b'{'; 1_200_000];
        exec.extend_from_slice(b"\n");
        exec.extend_from_slice(line(2, "exec after").as_bytes());
        fs::write(directory.path().join("exec.log"), exec).unwrap();
        let mut query = request();
        query.limit = Some(1);
        let (entries, first) = all_pages(directory.path(), query);
        assert_eq!(first.total_matches, 5);
        assert!(first.unreadable_records);
        let lines: Vec<_> = entries.iter().map(|entry| entry.line.as_str()).collect();
        assert!(lines.contains(&"2026-09-18T08:00:01Z after the flood"));
        assert!(lines.contains(&"exec after"));
        assert!(lines.contains(&"[Execution log record over 1 MiB omitted]"));
        let truncated: Vec<_> = lines
            .iter()
            .filter(|line| line.ends_with("[record over 1 MiB truncated]"))
            .collect();
        assert_eq!(
            truncated.len(),
            2,
            "{:?}",
            lines.iter().map(|line| line.len()).collect::<Vec<_>>()
        );
        assert!(truncated.iter().all(|line| line.len() < 70 * 1024));
        let mut search = request();
        search.query = Some("after the flood".into());
        assert_eq!(
            read(directory.path(), search, "dev", "pc", "Desktop")
                .unwrap()
                .total_matches,
            1
        );
    }
    #[test]
    fn guest_console_timestamps_are_labelled() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("kernel.log"),
            "2020-01-01T00:00:00Z forged by the guest\n",
        )
        .unwrap();
        fs::write(
            directory.path().join("runtime.log"),
            "2026-09-18T08:00:00Z runtime\n",
        )
        .unwrap();
        fs::write(directory.path().join("exec.log"), line(1, "exec")).unwrap();
        let page = read(directory.path(), request(), "dev", "pc", "Desktop").unwrap();
        for entry in &page.entries {
            assert_eq!(
                entry.guest_timestamp,
                entry.source == "kernel",
                "{}",
                entry.line
            );
        }
        let json = serde_json::to_value(&page).unwrap();
        let kernel = json["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["source"] == "kernel")
            .unwrap();
        assert_eq!(kernel["guestTimestamp"], true);
        assert!(json["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["source"] != "kernel")
            .all(|entry| entry.get("guestTimestamp").is_none()));
        let mut context = request();
        context.around_id = Some(kernel["id"].as_str().unwrap().into());
        let around = read(directory.path(), context, "dev", "pc", "Desktop").unwrap();
        assert!(around.entries.iter().any(|entry| entry.guest_timestamp));
    }
    #[test]
    fn time_and_source_filters_cover_rotated_plain_text_with_estimated_timestamps() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("runtime.log.7"),
            "2026-09-18T08:00:00+00:00 runtime error\n",
        )
        .unwrap();
        fs::write(directory.path().join("kernel.log"), "kernel boot\n").unwrap();
        let mut query = request();
        query.source = Some("runtime".into());
        query.since = Some("2026-09-18T09:00:00+02:00".into());
        query.until = Some("2026-09-18T09:00:00Z".into());
        let page = read(directory.path(), query, "dev", "pc", "Desktop").unwrap();
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.entries[0].source, "runtime");
        assert!(page.timestamp_estimated);
        let mut query = request();
        query.since = Some("invalid".into());
        assert!(read(directory.path(), query, "dev", "pc", "Desktop").is_err());
    }
}
