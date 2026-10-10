//! Downloads macOS restore images (`.ipsw`) into a shared directory, resuming
//! an interrupted download with an HTTP Range request.
use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

const EXTENSION: &str = "ipsw";
const PARTIAL_SUFFIX: &str = ".partial";
/// How long a download may receive nothing before the attempt fails and can resume.
const STALL: Duration = if cfg!(test) {
    Duration::from_millis(600)
} else {
    Duration::from_secs(60)
};
/// How often a waiting download looks for cancellation.
const POLL: Duration = if cfg!(test) {
    Duration::from_millis(100)
} else {
    Duration::from_secs(2)
};
const ATTEMPTS: usize = 4;
const RETRY_DELAY: Duration = if cfg!(test) {
    Duration::from_millis(10)
} else {
    Duration::from_secs(2)
};

/// Images an installation is reading. Pruning leaves them alone.
static IN_USE: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

pub(super) struct InUse(PathBuf);

impl InUse {
    pub(super) fn new(path: &Path) -> Self {
        if let Ok(mut paths) = IN_USE.lock() {
            paths.push(path.to_path_buf());
        }
        Self(path.to_path_buf())
    }
}

impl Drop for InUse {
    fn drop(&mut self) {
        if let Ok(mut paths) = IN_USE.lock() {
            if let Some(index) = paths.iter().position(|path| *path == self.0) {
                paths.swap_remove(index);
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum DownloadError {
    Cancelled,
    Failed(String),
}

impl From<String> for DownloadError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

/// The local file name for an image URL: its last path component.
pub(super) fn file_name(url: &str) -> Result<String, String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| "The macOS download address is invalid.")?;
    let name = parsed
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .unwrap_or_default();
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        && !name.starts_with('.')
        && Path::new(name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case(EXTENSION));
    if valid {
        Ok(name.to_string())
    } else {
        Err("The macOS download address is not a restore image.".into())
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Plan {
    /// Keep the partial file and append the body.
    Append { total: Option<u64> },
    /// Discard the partial file and write the body from the start.
    Restart { total: Option<u64> },
    /// The partial file already holds the whole image.
    Complete,
    /// The server rejected the range: start over without one.
    RetryWithoutRange,
}

/// `Content-Range: bytes 100-199/200` as (first, total).
fn parse_content_range(value: &str) -> Option<(u64, u64)> {
    let range = value.trim().strip_prefix("bytes ")?;
    let (span, total) = range.split_once('/')?;
    let (first, _) = span.split_once('-')?;
    Some((first.trim().parse().ok()?, total.trim().parse().ok()?))
}

/// What to do with a response, given how many bytes are already on disk.
fn plan_response(
    status: u16,
    existing: u64,
    content_length: Option<u64>,
    content_range: Option<&str>,
) -> Result<Plan, String> {
    match status {
        206 => {
            let (first, total) = content_range
                .and_then(parse_content_range)
                .ok_or("The macOS download server sent an invalid range.")?;
            if first != existing {
                return Err("The macOS download server resumed at the wrong position.".into());
            }
            Ok(Plan::Append { total: Some(total) })
        }
        200 => Ok(Plan::Restart {
            total: content_length,
        }),
        416 => {
            let total = content_range
                .and_then(|value| value.trim().strip_prefix("bytes */"))
                .and_then(|total| total.trim().parse::<u64>().ok());
            if existing > 0 && total == Some(existing) {
                Ok(Plan::Complete)
            } else {
                Ok(Plan::RetryWithoutRange)
            }
        }
        status => Err(format!("The macOS download server answered {status}.")),
    }
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .tcp_keepalive(Duration::from_secs(30))
        .build()
        .map_err(|_| "Silo could not prepare the macOS download.".into())
}

/// Downloads `url` into `dir` and returns the finished file. A finished file
/// is returned as it is. `progress` receives (bytes so far, total when known);
/// `cancelled` is polled while data arrives.
pub(super) fn download(
    url: &str,
    dir: &Path,
    cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<PathBuf, DownloadError> {
    let name = file_name(url)?;
    let (target, fresh) = fetch(url, dir, &name, cancelled, progress)?;
    if fresh {
        prune_other_images(dir, &name);
    }
    Ok(target)
}

/// Downloads `url` into `dir` as `name` with the same resuming and retries as
/// `download`, leaving every other file in `dir` alone.
pub(super) fn download_as(
    url: &str,
    dir: &Path,
    name: &str,
    cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<PathBuf, DownloadError> {
    fetch(url, dir, name, cancelled, progress).map(|(target, _)| target)
}

/// The finished file and whether this call downloaded it.
fn fetch(
    url: &str,
    dir: &Path,
    name: &str,
    cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(PathBuf, bool), DownloadError> {
    let target = dir.join(name);
    if target.is_file() {
        return Ok((target, false));
    }
    fs::create_dir_all(dir)
        .map_err(|error| super::store::io_error("prepare the download", &error))?;
    let partial = dir.join(format!("{name}{PARTIAL_SUFFIX}"));
    let client = client()?;
    let mut last_error = String::new();
    for attempt in 0..ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(RETRY_DELAY);
        }
        if cancelled() {
            return Err(DownloadError::Cancelled);
        }
        match attempt_download(&client, url, &partial, cancelled, progress) {
            Ok(()) => {
                fs::rename(&partial, &target)
                    .map_err(|error| super::store::io_error("finish the download", &error))?;
                return Ok((target, true));
            }
            Err(Attempt::Cancelled) => return Err(DownloadError::Cancelled),
            Err(Attempt::Fatal(message)) => return Err(DownloadError::Failed(message)),
            Err(Attempt::Retry(message)) => last_error = message,
        }
    }
    Err(DownloadError::Failed(last_error))
}

enum Attempt {
    Cancelled,
    /// The same request could succeed later: keep the partial file and try again.
    Retry(String),
    Fatal(String),
}

fn attempt_download(
    client: &reqwest::Client,
    url: &str,
    partial: &Path,
    cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(), Attempt> {
    tauri::async_runtime::block_on(attempt_async(client, url, partial, cancelled, progress))
}

/// Waits for `future`, noticing cancellation every `POLL` and giving up after
/// `STALL` without it finishing.
async fn patiently<F: std::future::Future>(
    future: F,
    cancelled: &dyn Fn() -> bool,
) -> Result<F::Output, Attempt> {
    let mut future = std::pin::pin!(future);
    let mut idle = Duration::ZERO;
    loop {
        match tokio::time::timeout(POLL, future.as_mut()).await {
            Ok(output) => return Ok(output),
            Err(_) => {
                if cancelled() {
                    return Err(Attempt::Cancelled);
                }
                idle += POLL;
                if idle >= STALL {
                    return Err(Attempt::Retry(
                        "The macOS download stopped receiving data.".into(),
                    ));
                }
            }
        }
    }
}

async fn attempt_async(
    client: &reqwest::Client,
    url: &str,
    partial: &Path,
    cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(), Attempt> {
    let interrupted = |error: &dyn std::fmt::Display| {
        Attempt::Retry(format!("The macOS download was interrupted: {error}"))
    };
    let mut existing = fs::metadata(partial).map(|m| m.len()).unwrap_or(0);
    let mut use_range = existing > 0;
    let (mut response, plan) = loop {
        let mut request = client.get(url);
        if use_range {
            request = request.header(reqwest::header::RANGE, format!("bytes={existing}-"));
        }
        let response = patiently(request.send(), cancelled)
            .await?
            .map_err(|error| interrupted(&error))?;
        let header = |name| {
            response
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string)
        };
        let length = header(reqwest::header::CONTENT_LENGTH).and_then(|value| value.parse().ok());
        let range = header(reqwest::header::CONTENT_RANGE);
        let plan = plan_response(
            response.status().as_u16(),
            existing,
            length,
            range.as_deref(),
        )
        .map_err(Attempt::Fatal)?;
        match plan {
            Plan::RetryWithoutRange => {
                let _ = fs::remove_file(partial);
                existing = 0;
                use_range = false;
            }
            plan => break (response, plan),
        }
    };
    let total = match plan {
        Plan::Complete => return Ok(()),
        Plan::Append { total } | Plan::Restart { total } => total,
        Plan::RetryWithoutRange => unreachable!("handled in the request loop"),
    };
    let appending = matches!(plan, Plan::Append { .. });
    let mut file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(appending)
        .truncate(!appending)
        .open(partial)
        .map_err(|error| Attempt::Fatal(super::store::io_error("save the download", &error)))?;
    let mut written = if appending { existing } else { 0 };
    progress(written, total);
    loop {
        if cancelled() {
            return Err(Attempt::Cancelled);
        }
        let chunk = patiently(response.chunk(), cancelled)
            .await?
            .map_err(|error| interrupted(&error))?;
        let Some(chunk) = chunk else { break };
        file.write_all(&chunk)
            .map_err(|error| Attempt::Fatal(super::store::io_error("save the download", &error)))?;
        written += chunk.len() as u64;
        progress(written, total);
    }
    drop(file);
    match total {
        Some(total) if written < total => Err(interrupted(&"the connection closed early")),
        Some(total) if written > total => {
            let _ = fs::remove_file(partial);
            Err(Attempt::Fatal(
                "The macOS download is larger than expected. Try again.".into(),
            ))
        }
        _ => Ok(()),
    }
}

/// Removes the other restore images, which a newer download supersedes.
fn prune_other_images(dir: &Path, keep: &str) {
    let in_use: HashSet<PathBuf> = IN_USE
        .lock()
        .map(|paths| paths.iter().cloned().collect())
        .unwrap_or_default();
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let file = entry.file_name();
        let Some(file) = file.to_str() else { continue };
        let image = file.strip_suffix(PARTIAL_SUFFIX).unwrap_or(file);
        let is_image = Path::new(image)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case(EXTENSION));
        if is_image && image != keep && !in_use.contains(&entry.path()) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Removes the cached restore image of macOS `build` from `dir`, the finished file and a
/// partial one, except an image an installation is reading. The images in use are checked
/// while their list is locked, which is also the lock an installation takes before it
/// reads one, so an image cannot be removed from under it or claimed while it is removed.
/// Returns the names removed with their sizes.
pub(super) fn remove_for_build(dir: &Path, build: &str) -> Vec<(String, u64)> {
    let mut removed = Vec::new();
    if build.is_empty() {
        return removed;
    }
    let Ok(in_use) = IN_USE.lock() else {
        return removed;
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return removed;
    };
    for entry in entries.flatten() {
        let file = entry.file_name();
        let Some(file) = file.to_str() else { continue };
        let image = file.strip_suffix(PARTIAL_SUFFIX).unwrap_or(file);
        let is_image = Path::new(image)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case(EXTENSION));
        // Apple's names carry the build as one `_`-separated part: UniversalMac_26.6.2_25G83_Restore.ipsw.
        let of_build = image
            .trim_end_matches(&format!(".{EXTENSION}"))
            .split('_')
            .any(|part| part == build);
        let busy = in_use.iter().any(|path| {
            path.file_name()
                .is_some_and(|name| name.to_str() == Some(image))
        });
        if is_image && of_build && !busy {
            let size = entry.metadata().map_or(0, |meta| meta.len());
            if fs::remove_file(entry.path()).is_ok() {
                removed.push((file.to_string(), size));
            }
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader},
        net::TcpListener,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
    };

    const IMAGE: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";

    #[test]
    fn file_names_come_from_the_url() {
        assert_eq!(
            file_name("https://updates.cdn-apple.com/a/b/UniversalMac_26.6.2_25G83_Restore.ipsw")
                .unwrap(),
            "UniversalMac_26.6.2_25G83_Restore.ipsw"
        );
        for url in [
            "https://example.com/",
            "https://example.com/image.zip",
            "https://example.com/.ipsw",
            "https://example.com/a%2F..%2Fb.ipsw",
            "not a url",
        ] {
            assert!(file_name(url).is_err(), "{url}");
        }
    }

    #[test]
    fn content_ranges_parse() {
        assert_eq!(parse_content_range("bytes 10-35/36"), Some((10, 36)));
        assert_eq!(parse_content_range("bytes */36"), None);
        assert_eq!(parse_content_range("items 1-2/3"), None);
    }

    #[test]
    fn responses_map_to_plans() {
        assert_eq!(
            plan_response(206, 10, Some(26), Some("bytes 10-35/36")),
            Ok(Plan::Append { total: Some(36) })
        );
        assert!(plan_response(206, 10, Some(26), Some("bytes 0-35/36")).is_err());
        assert!(plan_response(206, 10, Some(26), None).is_err());
        assert_eq!(
            plan_response(200, 10, Some(36), None),
            Ok(Plan::Restart { total: Some(36) })
        );
        assert_eq!(
            plan_response(416, 36, None, Some("bytes */36")),
            Ok(Plan::Complete)
        );
        assert_eq!(
            plan_response(416, 40, None, Some("bytes */36")),
            Ok(Plan::RetryWithoutRange)
        );
        assert!(plan_response(404, 0, None, None).is_err());
    }

    /// Serves `IMAGE` once per accepted connection. `ranges` decides whether
    /// the server honours Range; `truncate` closes the body early.
    struct Server {
        url: String,
        requests: Arc<Mutex<Vec<Option<String>>>>,
    }

    fn serve(ranges: bool, truncate: Option<usize>, connections: usize) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://{}/Test_1.0_Restore.ipsw",
            listener.local_addr().unwrap()
        );
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        std::thread::spawn(move || {
            for _ in 0..connections {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut range = None;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        range = Some(value.trim().trim_end_matches('-').to_string());
                    }
                }
                seen.lock().unwrap().push(range.clone());
                let start = range
                    .as_deref()
                    .filter(|_| ranges)
                    .and_then(|r| r.parse::<usize>().ok());
                let (head, body) = match start {
                    Some(start) if start >= IMAGE.len() => (
                        format!(
                            "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                            IMAGE.len() + 1
                        ),
                        &IMAGE[..0],
                    ),
                    Some(start) => (
                        format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            IMAGE.len() - 1,
                            IMAGE.len(),
                            IMAGE.len() - start
                        ),
                        &IMAGE[start..],
                    ),
                    None => (
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            IMAGE.len()
                        ),
                        IMAGE,
                    ),
                };
                let body = truncate.map_or(body, |n| &body[..n.min(body.len())]);
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body);
            }
        });
        Server { url, requests }
    }

    fn fetch(server: &Server, dir: &Path) -> Result<PathBuf, DownloadError> {
        download(&server.url, dir, &|| false, &mut |_, _| {})
    }

    #[test]
    fn a_fresh_download_is_renamed_when_complete() {
        let dir = tempfile::tempdir().unwrap();
        let server = serve(true, None, 1);
        let file = fetch(&server, dir.path()).unwrap();
        assert_eq!(fs::read(&file).unwrap(), IMAGE);
        assert!(!dir.path().join("Test_1.0_Restore.ipsw.partial").exists());
        assert_eq!(*server.requests.lock().unwrap(), vec![None]);
    }

    #[test]
    fn a_finished_image_is_not_downloaded_again() {
        let dir = tempfile::tempdir().unwrap();
        let server = serve(true, None, 0);
        fs::write(dir.path().join("Test_1.0_Restore.ipsw"), IMAGE).unwrap();
        fetch(&server, dir.path()).unwrap();
        assert!(server.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn a_206_response_appends_to_the_partial_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Test_1.0_Restore.ipsw.partial"),
            &IMAGE[..10],
        )
        .unwrap();
        let server = serve(true, None, 1);
        let file = fetch(&server, dir.path()).unwrap();
        assert_eq!(fs::read(file).unwrap(), IMAGE);
        assert_eq!(*server.requests.lock().unwrap(), vec![Some("10".into())]);
    }

    #[test]
    fn a_200_response_restarts_the_download() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Test_1.0_Restore.ipsw.partial"),
            b"stale bytes",
        )
        .unwrap();
        let server = serve(false, None, 1);
        let file = fetch(&server, dir.path()).unwrap();
        assert_eq!(fs::read(file).unwrap(), IMAGE);
    }

    #[test]
    fn an_early_close_resumes_where_it_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let server = serve(true, Some(12), 3);
        let file = fetch(&server, dir.path()).unwrap();
        assert_eq!(fs::read(file).unwrap(), IMAGE);
        assert_eq!(
            *server.requests.lock().unwrap(),
            vec![None, Some("12".into()), Some("24".into())]
        );
    }

    #[test]
    fn a_download_that_never_completes_fails_and_keeps_the_partial_file() {
        let dir = tempfile::tempdir().unwrap();
        let server = serve(true, Some(5), ATTEMPTS);
        let result = fetch(&server, dir.path());
        assert!(matches!(result, Err(DownloadError::Failed(_))));
        let partial = fs::read(dir.path().join("Test_1.0_Restore.ipsw.partial")).unwrap();
        assert_eq!(partial, &IMAGE[..5 * ATTEMPTS]);
        assert!(!dir.path().join("Test_1.0_Restore.ipsw").exists());
    }

    #[test]
    fn a_partial_file_the_server_cannot_resume_is_discarded() {
        let dir = tempfile::tempdir().unwrap();
        let partial = dir.path().join("Test_1.0_Restore.ipsw.partial");
        fs::write(&partial, vec![b'x'; IMAGE.len() + 5]).unwrap();
        let server = serve(true, None, 2);
        let file = fetch(&server, dir.path()).unwrap();
        assert_eq!(fs::read(file).unwrap(), IMAGE);
        assert_eq!(server.requests.lock().unwrap()[1], None);
    }

    /// Accepts connections and never answers, keeping them open.
    fn silent_server(connections: usize) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://{}/Test_1.0_Restore.ipsw",
            listener.local_addr().unwrap()
        );
        let handle = std::thread::spawn(move || {
            let mut held = Vec::new();
            for _ in 0..connections {
                match listener.accept() {
                    Ok((stream, _)) => held.push(stream),
                    Err(_) => return,
                }
            }
            std::thread::sleep(Duration::from_secs(3));
        });
        (url, handle)
    }

    #[test]
    fn a_stalled_connection_fails_the_download() {
        let dir = tempfile::tempdir().unwrap();
        let (url, _server) = silent_server(ATTEMPTS);
        let started = std::time::Instant::now();
        let result = download(&url, dir.path(), &|| false, &mut |_, _| {});
        assert!(matches!(result, Err(DownloadError::Failed(_))));
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(!dir.path().join("Test_1.0_Restore.ipsw").exists());
    }

    #[test]
    fn cancelling_a_stalled_download_is_noticed_quickly() {
        let dir = tempfile::tempdir().unwrap();
        let (url, _server) = silent_server(1);
        let begun = std::time::Instant::now();
        let result = download(
            &url,
            dir.path(),
            &|| begun.elapsed() > Duration::from_millis(250),
            &mut |_, _| {},
        );
        assert_eq!(result, Err(DownloadError::Cancelled));
        assert!(begun.elapsed() < Duration::from_millis(550));
    }

    #[test]
    fn cancelling_stops_and_keeps_the_partial_file() {
        let dir = tempfile::tempdir().unwrap();
        let server = serve(true, None, 1);
        let stop = AtomicBool::new(false);
        let result = download(
            &server.url,
            dir.path(),
            &|| stop.load(Ordering::SeqCst),
            &mut |written, _| {
                if written > 0 {
                    stop.store(true, Ordering::SeqCst);
                }
            },
        );
        assert_eq!(result, Err(DownloadError::Cancelled));
        assert!(!dir.path().join("Test_1.0_Restore.ipsw").exists());
    }

    #[test]
    fn a_new_image_supersedes_the_others_except_those_in_use() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("Old_0.9_Restore.ipsw");
        let busy = dir.path().join("Busy_0.8_Restore.ipsw");
        fs::write(&old, b"old").unwrap();
        fs::write(&busy, b"busy").unwrap();
        fs::write(dir.path().join("Older_0.7_Restore.ipsw.partial"), b"x").unwrap();
        fs::write(dir.path().join("notes.txt"), b"keep").unwrap();
        let _guard = InUse::new(&busy);
        let server = serve(true, None, 1);
        let file = fetch(&server, dir.path()).unwrap();
        assert!(file.exists());
        assert!(!old.exists());
        assert!(busy.exists());
        assert!(!dir.path().join("Older_0.7_Restore.ipsw.partial").exists());
        assert!(dir.path().join("notes.txt").exists());
    }

    #[test]
    fn the_image_of_a_build_is_removed_complete_and_partial_but_never_while_in_use() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str| fs::write(dir.path().join(name), b"image").unwrap();
        write("UniversalMac_26.6.2_25G83_Restore.ipsw");
        write("UniversalMac_26.6.2_25G83_Restore.ipsw.partial");
        write("UniversalMac_27.0_26A1_Restore.ipsw");
        write("UniversalMac_26.6.2_25G83_Restore.txt");
        write("notes.txt");
        // A longer build that merely starts with the same characters is another image.
        write("UniversalMac_26.6.2_25G830_Restore.ipsw");
        let busy = dir.path().join("UniversalMac_26.6.2_25G83_Restore.ipsw");
        let guard = InUse::new(&busy);
        assert!(remove_for_build(dir.path(), "25G83").is_empty());
        assert!(busy.exists());
        // Once nothing reads it, the image and its partial file go.
        drop(guard);
        let mut removed: Vec<String> = remove_for_build(dir.path(), "25G83")
            .into_iter()
            .map(|(name, size)| {
                assert_eq!(size, 5);
                name
            })
            .collect();
        removed.sort();
        assert_eq!(
            removed,
            [
                "UniversalMac_26.6.2_25G83_Restore.ipsw",
                "UniversalMac_26.6.2_25G83_Restore.ipsw.partial"
            ]
        );
        let mut left: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "UniversalMac_26.6.2_25G830_Restore.ipsw",
                "UniversalMac_26.6.2_25G83_Restore.txt",
                "UniversalMac_27.0_26A1_Restore.ipsw",
                "notes.txt"
            ]
        );
        assert!(remove_for_build(dir.path(), "").is_empty());
        assert!(remove_for_build(&dir.path().join("missing"), "25G83").is_empty());
    }
}
