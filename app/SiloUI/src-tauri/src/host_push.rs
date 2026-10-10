//! Explicit host push: only committed objects cross the guest boundary.
use crate::runtime::{self, RuntimePaths};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::os::{fd::AsRawFd, unix::process::CommandExt};
use std::{
    collections::HashMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager};

/// Repository discovery per computer: the last finished read (with its start time)
/// and whether a background read is in flight.
#[derive(Default)]
struct Discovery {
    last: Option<(Instant, Result<Vec<Value>, String>)>,
    running: bool,
    generation: u64,
    /// The last rows predate a change to a repository; they are served only when a newer
    /// read does not finish in time.
    stale: bool,
    /// What a caller was last given while a newer read was still in flight.
    served: Option<Result<Vec<Value>, String>>,
}
impl Discovery {
    /// Whether this finished read differs from what callers were given in the meantime.
    fn differs_from_served(&mut self) -> bool {
        match (self.served.take(), &self.last) {
            (Some(served), Some((_, result))) => served != *result,
            _ => false,
        }
    }
    fn invalidate(&mut self) {
        self.stale = true;
        self.generation = self.generation.wrapping_add(1);
    }
    fn finish(&mut self, generation: u64, started: Instant, result: Result<Vec<Value>, String>) {
        if generation == self.generation {
            self.last = Some((started, result));
            self.stale = false;
        }
        self.running = false;
    }
}
static NOTIFY_APP: OnceLock<tauri::AppHandle> = OnceLock::new();
/// Lets a finished discovery tell the UI when it changes rows a state read already served.
pub(crate) fn install(app: &tauri::AppHandle) {
    let _ = NOTIFY_APP.set(app.clone());
}
type Discoveries = (Mutex<HashMap<String, Discovery>>, std::sync::Condvar);
static DISCOVERIES: OnceLock<Discoveries> = OnceLock::new();
fn discoveries() -> &'static Discoveries {
    DISCOVERIES.get_or_init(Default::default)
}
static RESULTS: OnceLock<Mutex<HashMap<String, (Value, Instant)>>> = OnceLock::new();
fn results() -> &'static Mutex<HashMap<String, (Value, Instant)>> {
    RESULTS.get_or_init(|| Mutex::new(HashMap::new()))
}
pub(crate) fn operations() -> Vec<Value> {
    results()
        .lock()
        .map(|r| {
            r.values()
                .filter(|(value, at)| {
                    value["status"] != "succeeded" || at.elapsed() < Duration::from_secs(4)
                })
                .map(|(value, _)| value.clone())
                .collect()
        })
        .unwrap_or_default()
}
fn dismiss_result(entries: &mut HashMap<String, (Value, Instant)>, key: &str) {
    if entries
        .get(key)
        .is_some_and(|(value, _)| matches!(value["status"].as_str(), Some("failed" | "succeeded")))
    {
        entries.remove(key);
    }
}

#[tauri::command]
pub async fn dismiss_repository_push(
    app: tauri::AppHandle,
    computer: String,
    repository_path: String,
) -> Result<(), String> {
    if let Some((device, computer)) = crate::remote_access::target(&computer)? {
        return tauri::async_runtime::spawn_blocking(move || {
            crate::remote::call_remote(
                &app,
                &device,
                "repository.dismiss",
                json!({"computerId":computer,"path":repository_path}),
            )
            .map(|_| ())
        })
        .await
        .map_err(|_| "Remote repository request failed.".to_string())?;
    }
    crate::host_push_operations::dismiss(&app, &computer, &repository_path)?;
    let mut entries = results().lock().map_err(|_| "Push state unavailable.")?;
    dismiss_result(&mut entries, &format!("{computer}\0{repository_path}"));
    drop(entries);
    let _ = app.emit("silo://application-state-changed", ());
    Ok(())
}

fn guest(paths: &RuntimePaths, name: &str, script: &str, args: &[&str]) -> Result<String, String> {
    guest_within(paths, name, script, args, 30)
}
/// `seconds` bounds the guest command; the host waits a little longer.
fn guest_within(
    paths: &RuntimePaths,
    name: &str,
    script: &str,
    args: &[&str],
    seconds: u64,
) -> Result<String, String> {
    let user = crate::working_account::inspect_user(paths, name)?;
    let mut command = vec![
        "exec".into(),
        name.into(),
        "--user".into(),
        user.into(),
        "--env".into(),
        format!("USER={user}"),
        "--env".into(),
        format!("LOGNAME={user}"),
        "--no-start".into(),
        "--no-tty".into(),
        "--quiet".into(),
        "--workdir".into(),
        "/".into(),
        "--timeout".into(),
        format!("{seconds}s"),
        "--".into(),
        "sh".into(),
        "-c".into(),
        script.into(),
        "silo-host-push".into(),
    ];
    command.extend(args.iter().map(|s| s.to_string()));
    runtime::run_msb(paths, &command, Duration::from_secs(seconds + 15))
        .map(|o| o.stdout)
        .map_err(|_| "Could not read committed repository data from the computer.".into())
}
/// The system store carries administrator-installed and updated roots (for
/// example TLS-inspecting proxies); the bundled file is only a fallback.
fn linux_ca_bundle(support: &Path) -> PathBuf {
    ca_bundle_from(
        &[
            Path::new("/etc/ssl/certs/ca-certificates.crt"),
            Path::new("/etc/pki/tls/certs/ca-bundle.crt"),
        ],
        support,
    )
}
fn ca_bundle_from(system: &[&Path], support: &Path) -> PathBuf {
    system
        .iter()
        .find(|path| fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() > 0))
        .map(|path| path.to_path_buf())
        .unwrap_or_else(|| support.join("ssl/cacert.pem"))
}
fn valid_path(path: &str) -> bool {
    crate::host_push_transport::valid_repository_path(path)
}
fn repository(url: &str) -> Result<String, String> {
    let name = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("git@github.com:"))
        .ok_or("Choose a GitHub origin repository before pushing.")?;
    let name = name.strip_suffix(".git").unwrap_or(name);
    if !valid_repository_name(name) {
        return Err("Invalid GitHub origin repository.".into());
    }
    Ok(name.into())
}
fn valid_repository_name(name: &str) -> bool {
    name.split('/').count() == 2
        && name.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
}
fn valid_commit(commit: &str) -> bool {
    matches!(commit.len(), 40 | 64) && commit.bytes().all(|c| c.is_ascii_hexdigit())
}
/// The repository, branch and commit the user confirmed. A push publishes
/// exactly this commit to this branch of this repository, or nothing
/// (owner decision 1).
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub(crate) struct PushTarget {
    pub(crate) repository: String,
    pub(crate) branch: String,
    pub(crate) commit: String,
}
impl PushTarget {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !valid_repository_name(&self.repository) {
            return Err("Invalid GitHub repository for this push.".into());
        }
        if self.branch.is_empty()
            || self.branch.len() > 255
            || self.branch.starts_with('-')
            || self.branch.chars().any(char::is_control)
        {
            return Err("Invalid branch for this push.".into());
        }
        if !valid_commit(&self.commit) {
            return Err("Invalid commit for this push.".into());
        }
        Ok(())
    }
}
const TARGET_CHANGED: &str =
    "The repository changed after you confirmed the push. Review it and push again.";
const UPDATE_TRACKING_REF: &str = r#"set -eu
origin=$(git -C "$1" remote get-url origin) || exit 0
[ "$origin" = "$5" ] || exit 0
git -C "$1" update-ref --no-deref "$2" "$3" "$4"
"#;
/// Rows at most this old are served without reading the guest again.
const DISCOVERY_FRESH: Duration = Duration::from_secs(60);
/// A state refresh waits this long for a computer's first discovery; later refreshes
/// never wait, so a slow or hostile guest cannot stall them.
const DISCOVERY_FIRST_WAIT: Duration = Duration::from_secs(3);
/// Guest time limit for one discovery; an explicit refresh waits for it.
const DISCOVERY_SECONDS: u64 = 20;

/// Starts a background read for a computer unless its rows are current or a read is in flight.
fn ensure_running(
    entries: &mut HashMap<String, Discovery>,
    paths: &RuntimePaths,
    name: &str,
    key: &str,
) {
    let entry = entries.entry(key.to_owned()).or_default();
    if entry.running {
        return;
    }
    entry.running = true;
    let generation = entry.generation;
    let (paths, name, key) = (paths.clone(), name.to_owned(), key.to_owned());
    thread::spawn(move || {
        let started = Instant::now();
        let result = contain_panic(|| discover_uncached(&paths, &name));
        let (lock, changed) = discoveries();
        let mut entries = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if entries.len() > 64 {
            entries.retain(|other, entry| entry.running || *other == key);
        }
        let entry = entries.entry(key).or_default();
        entry.finish(generation, started, result);
        let update = entry.differs_from_served();
        drop(entries);
        changed.notify_all();
        if update {
            if let Some(app) = NOTIFY_APP.get() {
                let _ = app.emit("silo://application-state-changed", ());
            }
        }
    });
}

/// Begins the background reads of the given running computers (`(name, computer id)`) so
/// they proceed together while the caller collects their rows with `discover_until`.
pub(crate) fn prefetch<'a>(
    paths: &RuntimePaths,
    computers: impl IntoIterator<Item = (&'a str, &'a str)>,
) {
    let (lock, _) = discoveries();
    let mut entries = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    for (name, computer_id) in computers {
        let key = format!("{}:{computer_id}", paths.home.display());
        let current = entries.get(&key).is_some_and(|entry| {
            !entry.stale
                && entry
                    .last
                    .as_ref()
                    .is_some_and(|(started, _)| started.elapsed() < DISCOVERY_FRESH)
        });
        if !current {
            ensure_running(&mut entries, paths, name, &key);
        }
    }
}

/// The deadline by which a state refresh stops waiting for first discoveries.
pub(crate) fn first_discovery_deadline() -> Instant {
    Instant::now() + DISCOVERY_FIRST_WAIT
}

/// Repositories of a running computer. Reads run in the background, one per computer at a
/// time; callers get the last known rows while a newer read is in flight.
/// `refresh` (the user's Refresh) waits for a read that started after the call.
pub(crate) fn discover(
    paths: &RuntimePaths,
    name: &str,
    computer_id: &str,
    refresh: bool,
) -> Result<Vec<Value>, String> {
    discover_until(
        paths,
        name,
        computer_id,
        refresh,
        first_discovery_deadline(),
    )
}

/// `discover` where a non-refresh call waits for a first (or post-change) read only until
/// `first_deadline`, so several computers can share one wait.
pub(crate) fn discover_until(
    paths: &RuntimePaths,
    name: &str,
    computer_id: &str,
    refresh: bool,
    first_deadline: Instant,
) -> Result<Vec<Value>, String> {
    let requested = Instant::now();
    let key = format!("{}:{computer_id}", paths.home.display());
    let (lock, changed) = discoveries();
    let wait_until = if refresh {
        requested + Duration::from_secs(DISCOVERY_SECONDS + 20)
    } else {
        first_deadline
    };
    let mut entries = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    loop {
        let entry = entries.entry(key.clone()).or_default();
        if let Some((started, result)) = &entry.last {
            let current = if refresh {
                *started >= requested
            } else {
                !entry.stale && started.elapsed() < DISCOVERY_FRESH
            };
            if current {
                return result.clone();
            }
        }
        ensure_running(&mut entries, paths, name, &key);
        let entry = entries.get_mut(&key).expect("entry exists");
        if !refresh && !entry.stale {
            if let Some((_, result)) = &entry.last {
                let result = result.clone();
                entry.served = Some(result.clone());
                return result;
            }
        }
        let now = Instant::now();
        if now >= wait_until {
            let result = match &entry.last {
                Some((_, result)) => result.clone(),
                None => Ok(Vec::new()),
            };
            entry.served = Some(result.clone());
            return result;
        }
        entries = changed
            .wait_timeout(entries, wait_until - now)
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .0;
    }
}

/// A panicking discovery still reports a result, so its entry stops running.
fn contain_panic(work: impl FnOnce() -> Result<Vec<Value>, String>) -> Result<Vec<Value>, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
        .unwrap_or_else(|_| Err("Repository discovery failed unexpectedly.".into()))
}
// The guest deadline and runtime output budget bound discovery. An entry-count
// cutoff discards every result when a computer contains many Git worktrees.
// Dependency and cache trees are skipped; they hold no repositories to push.
const DISCOVER_REPOSITORIES: &str = r#"find "$1" \( -name node_modules -o -name .venv -o -name __pycache__ -o -name .tox -o -name .gradle -o -name .pnpm-store \) -prune -o -name .git -prune -print 2>/dev/null | while IFS= read -r directory; do
p=${directory%/.git}
branch=$(git -C "$p" symbolic-ref --quiet --short HEAD) || continue
counts=$(git -C "$p" rev-list --left-right --count HEAD..."refs/remotes/origin/$branch" 2>/dev/null) || counts="$(git -C "$p" rev-list --count HEAD --not --remotes=origin) 0"
dirty=$(git -C "$p" status --porcelain --untracked-files=no 2>/dev/null) || { echo 'Cannot read repository working tree status' >&2; exit 1; }
head=$(git -C "$p" rev-parse --verify --quiet HEAD) || head=
origin=$(git -C "$p" remote get-url origin 2>/dev/null) || origin=
printf '%s\000%s\000%s\000%s\000%s\000%s\000' "$p" "$branch" "$counts" "$dirty" "$head" "$origin"
done"#;
const DISCOVERY_FIELDS: usize = 6;

fn discover_uncached(paths: &RuntimePaths, name: &str) -> Result<Vec<Value>, String> {
    Ok(discovered_rows(&guest_within(
        paths,
        name,
        DISCOVER_REPOSITORIES,
        &["/workspace"],
        DISCOVERY_SECONDS,
    )?))
}
/// Each row names the GitHub repository and head commit a push would publish,
/// so the user confirms them and the push is bound to them.
fn discovered_rows(output: &str) -> Vec<Value> {
    let fields: Vec<_> = output.split('\0').collect();
    let mut rows = Vec::new();
    for parts in fields.chunks_exact(DISCOVERY_FIELDS) {
        if !valid_path(parts[0]) || parts[1].chars().any(char::is_control) {
            continue;
        }
        let counts: Vec<u64> = parts[2]
            .split_whitespace()
            .filter_map(|n| n.parse().ok())
            .collect();
        if counts.len() != 2 {
            continue;
        }
        rows.push(json!({
            "path": parts[0],
            "branch": parts[1],
            "ahead": counts[0],
            "behind": counts[1],
            "dirty": !parts[3].is_empty(),
            "head": Some(parts[4]).filter(|head| valid_commit(head)),
            "repository": repository(parts[5]).ok(),
        }));
    }
    rows
}
struct HostGit {
    executable: PathBuf,
    directory: PathBuf,
    home: PathBuf,
    support: PathBuf,
    ssh_command: Option<String>,
    cache_lock_fd: Option<std::os::fd::RawFd>,
    /// When the push's GitHub credential stops working; no step runs past it.
    deadline: Option<Instant>,
}
const CANCELLED: &str = "Push cancelled. The branch was not updated.";
/// Precedes output relayed from the computer, which the guest controls.
const COMPUTER_OUTPUT: &str = "Output from the computer (not from Silo or GitHub):";
const CREDENTIAL_EXPIRED: &str =
    "The push took longer than its GitHub credential allows. Push again to continue.";
const STEP_TIMED_OUT: &str = "Git operation timed out. Check the remote before retrying.";
const FREE_SPACE_STOP: &str = "Host push stopped to preserve free disk space.";
const PROCESS_STATUS_UNAVAILABLE: &str = "Cannot read Git process status.";
/// Reported as an unknown result: GitHub may or may not have updated the branch.
const PUBLICATION_UNKNOWN: &str =
    "The push stopped while GitHub was receiving it. Check this branch on GitHub before pushing again.";
/// A final push interrupted by the host may already have updated the branch.
fn final_push_error(error: String) -> String {
    match error.lines().next() {
        Some(
            CANCELLED
            | CREDENTIAL_EXPIRED
            | STEP_TIMED_OUT
            | FREE_SPACE_STOP
            | PROCESS_STATUS_UNAVAILABLE,
        ) => PUBLICATION_UNKNOWN.into(),
        _ => error,
    }
}
/// Git and Git LFS read the GitHub token from this inherited pipe through the
/// standard credential-helper protocol. The token never appears in arguments,
/// the environment or a file, where other processes of the user could read it.
const CREDENTIAL_FD: libc::c_int = 3;
const CREDENTIAL_HELPER: &str = r#"!f() { test "$1" = get || exit 0; while IFS= read -r line && test -n "$line"; do :; done; IFS= read -r token <&3 || exit 0; printf 'username=x-access-token\npassword=%s\n' "$token"; }; f"#;
/// One answer per credential request; Git and Git LFS ask about once per
/// endpoint. The answers fit an empty pipe, so writing them never blocks.
fn credential_pipe(token: &str) -> Result<std::io::PipeReader, String> {
    use std::io::Write;
    const FAILED: &str = "Cannot prepare the GitHub credential for Git.";
    if token.is_empty() || token.len() > 1024 || token.bytes().any(|b| b <= b' ' || b == 127) {
        return Err("Invalid GitHub credential.".into());
    }
    let (reader, mut writer) = std::io::pipe().map_err(|_| FAILED)?;
    let answer = format!("{token}\n");
    for _ in 0..(4096 / answer.len()).min(32) {
        writer.write_all(answer.as_bytes()).map_err(|_| FAILED)?;
    }
    Ok(reader)
}
/// Credentials are offered only to the destination's origin, never to other hosts.
fn credential_origin(remote: &str) -> Result<&str, String> {
    let (scheme, rest) = remote
        .split_once("://")
        .ok_or("Invalid push destination.")?;
    let device = rest.split('/').next().unwrap_or_default();
    if !matches!(scheme, "https" | "http") || device.is_empty() {
        return Err("Invalid push destination.".into());
    }
    Ok(&remote[..scheme.len() + 3 + device.len()])
}
impl HostGit {
    fn run(&self, args: &[&str], token: Option<&str>, remote: &str) -> Result<String, String> {
        self.run_with_budget(args, token, remote, temporary_budget)
    }
    fn run_with_budget(
        &self,
        args: &[&str],
        token: Option<&str>,
        remote: &str,
        budget: impl Fn(&Path) -> Result<u64, String>,
    ) -> Result<String, String> {
        let mut command = Command::new(&self.executable);
        command.process_group(0);
        let file_budget = budget(&self.directory)? as libc::rlim_t;
        let cache_lock_fd = self.cache_lock_fd;
        let credential = token.map(credential_pipe).transpose()?;
        let credential_fd = credential.as_ref().map(|reader| reader.as_raw_fd());
        let mut settings = vec![
            "core.hooksPath=/dev/null".to_owned(),
            "core.fsmonitor=false".into(),
            // Production sources are ssh://; only the local test harness
            // publishes from file paths.
            if cfg!(test) {
                "protocol.file.allow=always".into()
            } else {
                "protocol.file.allow=never".into()
            },
            "protocol.ext.allow=never".into(),
            "http.followRedirects=false".into(),
            "credential.helper=".into(),
        ];
        if token.is_some() {
            settings.push(format!(
                "credential.{}.helper={CREDENTIAL_HELPER}",
                credential_origin(remote)?
            ));
        }
        settings.extend(
            [
                "fetch.fsckObjects=true",
                "transfer.fsckObjects=true",
                "gc.auto=0",
                "maintenance.auto=false",
            ]
            .map(String::from),
        );
        unsafe {
            command.pre_exec(move || {
                // Keep the cache locked until this Git process exits, even if
                // Silo crashes. The parent owns the file for the entire command.
                if let Some(mut fd) = cache_lock_fd {
                    if credential_fd.is_some() && fd == CREDENTIAL_FD {
                        // Move the lock aside; the credential pipe takes this number.
                        fd = libc::fcntl(fd, libc::F_DUPFD, CREDENTIAL_FD + 1);
                        if fd == -1 {
                            return Err(std::io::Error::last_os_error());
                        }
                    }
                    if libc::fcntl(fd, libc::F_SETFD, 0) == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                if let Some(fd) = credential_fd {
                    let result = if fd == CREDENTIAL_FD {
                        libc::fcntl(fd, libc::F_SETFD, 0)
                    } else {
                        libc::dup2(fd, CREDENTIAL_FD)
                    };
                    if result == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                let zero = libc::rlimit {
                    rlim_cur: 0,
                    rlim_max: 0,
                };
                if libc::setrlimit(libc::RLIMIT_CORE, &zero) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                let limit = libc::rlimit {
                    rlim_cur: file_budget,
                    rlim_max: file_budget,
                };
                if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        for setting in &settings {
            command.arg("-c").arg(setting);
        }
        command
            .args(args)
            .current_dir(&self.directory)
            .env_clear()
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", &self.home)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_EXEC_PATH", self.executable.parent().unwrap())
            .env(
                "PATH",
                format!(
                    "{}:/usr/bin:/bin",
                    self.executable.parent().unwrap().display()
                ),
            )
            .env("GIT_TEMPLATE_DIR", self.home.join("empty-templates"))
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_LFS_SKIP_SMUDGE", "1")
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(ssh_command) = &self.ssh_command {
            command
                .env("GIT_SSH_COMMAND", ssh_command)
                .env("GIT_SSH_VARIANT", "ssh");
        }
        if cfg!(target_os = "linux") {
            command.env("GIT_SSL_CAINFO", linux_ca_bundle(&self.support));
        }
        if token.is_some() {
            command
                .env("GIT_CONFIG_COUNT", "1")
                .env("GIT_CONFIG_KEY_0", "lfs.url")
                .env("GIT_CONFIG_VALUE_0", format!("{remote}/info/lfs"));
        }
        let mut child = command
            .spawn()
            .map_err(|_| "Bundled Git could not start. Repair Silo and retry.")?;
        // Only the Git process tree keeps the credential pipe open.
        drop(credential);
        let stdout = child.stdout.take().ok_or("Cannot capture Git output.")?;
        let stderr = child
            .stderr
            .take()
            .ok_or("Cannot capture Git diagnostics.")?;
        let output_reader = thread::spawn(move || read_bounded(stdout, 1024 * 1024));
        let diagnostic_reader = thread::spawn(move || read_bounded(stderr, 16_384));
        let step_deadline = Instant::now() + Duration::from_secs(1800);
        let deadline = self
            .deadline
            .map_or(step_deadline, |end| end.min(step_deadline));
        let mut space_check = Instant::now();
        let outcome = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Err(_) => break Err(PROCESS_STATUS_UNAVAILABLE),
                Ok(None) if runtime::operation_gate::cancel_requested() => break Err(CANCELLED),
                Ok(None) if Instant::now() >= deadline => {
                    break Err(if deadline < step_deadline {
                        CREDENTIAL_EXPIRED
                    } else {
                        STEP_TIMED_OUT
                    });
                }
                Ok(None) => {
                    if space_check.elapsed() >= Duration::from_secs(1) {
                        if budget(&self.directory).is_err() {
                            break Err(FREE_SPACE_STOP);
                        }
                        space_check = Instant::now();
                    }
                    thread::sleep(Duration::from_millis(25));
                }
            }
        };
        // The process group belongs only to this invocation. Close inherited
        // pipes on failure so a helper cannot leave the readers blocked.
        if outcome.as_ref().map_or(true, |status| !status.success()) {
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.wait();
        }
        let (output, overflow) = output_reader
            .join()
            .map_err(|_| "Cannot capture Git output.")??;
        let (diagnostic, _) = diagnostic_reader
            .join()
            .map_err(|_| "Cannot capture Git diagnostics.")??;
        let mut diagnostic = String::from_utf8_lossy(&diagnostic).into_owned();
        if let Some(token) = token {
            diagnostic = diagnostic.replace(token, "[redacted]").replace(
                &STANDARD.encode(format!("x-access-token:{token}")),
                "[redacted]",
            );
        }
        let diagnostic = transfer_diagnostic(diagnostic.as_bytes());
        let mut command_args = args;
        while command_args.first() == Some(&"-c") && command_args.len() >= 2 {
            command_args = &command_args[2..];
        }
        let stage = command_args
            .iter()
            .take(if command_args.first() == Some(&"lfs") {
                2
            } else {
                1
            })
            .copied()
            .collect::<Vec<_>>()
            .join(" ");
        // Stages reading from the computer relay text the guest controls. Keep it
        // out of the visible message and label it in the details.
        let from_computer = args.contains(&"silo-source");
        let diagnostic = if from_computer && !diagnostic.is_empty() {
            format!("{COMPUTER_OUTPUT} {diagnostic}")
        } else {
            diagnostic
        };
        match outcome {
            Ok(status) if !status.success() => {
                if stage == "push" && command_args.contains(&"--porcelain") {
                    // Only an explicit per-ref rejection proves no update.
                    // Missing status and remote failures can follow acceptance.
                    let rejected = !overflow
                        && std::str::from_utf8(&output).is_ok_and(|output| {
                            output.lines().any(|line| {
                                let fields: Vec<_> = line.splitn(3, '\t').collect();
                                matches!(fields.as_slice(), ["!", _, summary]
                                    if summary.starts_with("[rejected]")
                                        || summary.starts_with("[remote rejected]"))
                            })
                        });
                    if !rejected {
                        return Err(PUBLICATION_UNKNOWN.into());
                    }
                }
                // The first line is the summary; the rest becomes diagnostic details.
                return Err(if from_computer {
                    format!("Reading committed data from the computer failed (Git {stage}, {status}).\n{diagnostic}")
                } else {
                    format!("Git {stage} failed ({status}).\n{diagnostic}")
                });
            }
            Err(message) => return Err(format!("{message}\n{diagnostic}")),
            _ => {}
        }
        if overflow {
            return Err("Git returned too much output to verify safely.".into());
        }
        String::from_utf8(output).map_err(|_| "Invalid Git output.".into())
    }
}
fn read_bounded(mut input: impl Read, limit: usize) -> Result<(Vec<u8>, bool), String> {
    let mut retained = Vec::new();
    let mut overflow = false;
    let mut buffer = [0; 8192];
    loop {
        let count = input
            .read(&mut buffer)
            .map_err(|_| "Cannot read Git output.")?;
        if count == 0 {
            break;
        }
        let keep = count.min(limit - retained.len());
        retained.extend_from_slice(&buffer[..keep]);
        overflow |= keep < count;
    }
    Ok((retained, overflow))
}

fn temporary_budget(directory: &Path) -> Result<u64, String> {
    let path = std::ffi::CString::new(directory.as_os_str().as_encoded_bytes())
        .map_err(|_| "Invalid temporary directory.")?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return Err("Cannot check free space for host push.".into());
    }
    let stat = unsafe { stat.assume_init() };
    let available = (stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64);
    let budget = (available / 2).min(available.saturating_sub(1024 * 1024 * 1024));
    if budget == 0 {
        return Err("Free at least 1 GB of temporary disk space before pushing.".into());
    }
    Ok(budget)
}
// Drain stderr while retaining a bounded diagnostic so a noisy runtime cannot
// block the binary stream or consume unbounded memory.
fn transfer_diagnostic(mut input: impl Read) -> String {
    let mut retained = Vec::new();
    let mut buffer = [0; 4096];
    while let Ok(count) = input.read(&mut buffer) {
        if count == 0 {
            break;
        }
        let keep = count.min(16_384 - retained.len());
        retained.extend_from_slice(&buffer[..keep]);
    }
    String::from_utf8_lossy(&retained)
        .split_whitespace()
        .map(|word| {
            if word.contains('/')
                || word.contains('@')
                || word.contains('=')
                || word.to_ascii_lowercase().contains("token")
                || word.len() > 80
            {
                "[redacted]"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .take(2000)
        .collect()
}

fn require_running(paths: &RuntimePaths, computer: &str) -> Result<(), String> {
    let inspected = runtime::run_msb(
        paths,
        &[
            "inspect".into(),
            "--format".into(),
            "json".into(),
            computer.into(),
        ],
        Duration::from_secs(15),
    )
    .map_err(|_| "Cannot verify computer state.")?;
    let inspected: runtime::InspectedSandbox =
        serde_json::from_str(&inspected.stdout).map_err(|_| "Invalid computer state.")?;
    validate_running(&inspected, computer)
}
fn validate_running(inspected: &runtime::InspectedSandbox, computer: &str) -> Result<(), String> {
    runtime::ensure_managed(inspected).map_err(|e| e.to_string())?;
    if inspected.name != computer || !inspected.status.eq_ignore_ascii_case("running") {
        return Err("Start the computer before pushing its committed changes.".into());
    }
    Ok(())
}
fn perform(
    app: &tauri::AppHandle,
    computer: &str,
    path: &str,
    target: &PushTarget,
) -> Result<u64, String> {
    let _update = crate::updates::operation_guard()?;
    if !valid_path(path) {
        return Err("Choose a repository inside /workspace.".into());
    }
    target.validate()?;
    runtime::validate_name(computer).map_err(|e| e.to_string())?;
    let paths = runtime::runtime_paths(app)?;
    let metadata = runtime::read_metadata(&paths.metadata).map_err(|e| e.to_string())?;
    let computer_id = metadata
        .computers
        .iter()
        .find(|m| m.name() == computer)
        .map(|m| m.id().to_owned())
        .ok_or("Choose a managed Silo computer.")?;
    // Host-push reads and writes one computer's guest; it waits its turn for that computer.
    let read_guard = runtime::OPERATIONS
        .computer(
            &computer_id,
            computer,
            &format!("Reading repository in {computer}"),
        )
        .map_err(|e| e.to_string())?;
    runtime::shutdown::ensure_accepting_operations()?;
    require_running(&paths, computer)?;
    let origin = guest(
        &paths,
        computer,
        "git -C \"$1\" remote get-url origin",
        &[path],
    )?;
    drop(read_guard);
    // Authorize and push only the repository the user confirmed.
    if !repository(origin.trim())?.eq_ignore_ascii_case(&target.repository) {
        return Err(TARGET_CHANGED.into());
    }
    // Revoked when this function returns, whatever the outcome.
    let credential = crate::github::host_push_credential(app, computer, &target.repository)?;
    let guard = runtime::OPERATIONS
        .kind(runtime::operation_gate::OperationKind::Push)
        .computer(&computer_id, computer, &format!("Pushing from {computer}"))
        .map_err(|e| e.to_string())?;
    // A push can run for a long time; the user may stop it (and Quit may cancel it).
    guard.allow_cancel();
    let result = (|| {
        runtime::shutdown::ensure_accepting_operations()?;
        require_running(&paths, computer)?;
        let executable = crate::bundled_tools::directory(app)?.join("git");
        let support = app
            .path()
            .resource_dir()
            .map_err(|_| "Cannot locate Git support.")?
            .join("git-support");
        push_target(
            &paths,
            computer,
            &computer_id,
            path,
            target,
            credential.repository(),
            credential.token(),
            credential_deadline(credential.expires_at()),
            &executable,
            &support,
        )
    })();
    // A cancelled step reports its own failure; say what happened instead.
    result.map_err(|error| {
        if error != PUBLICATION_UNKNOWN && runtime::operation_gate::cancel_requested() {
            CANCELLED.into()
        } else {
            error
        }
    })
}
/// Stop a minute before GitHub rejects the credential.
fn credential_deadline(expires_at: Option<u64>) -> Option<Instant> {
    let expires_at = expires_at?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(Instant::now() + Duration::from_secs(expires_at.saturating_sub(now).saturating_sub(60)))
}

// The host owns all configuration and credentials. The source remote supplies
// Git/LFS data through their standard protocols, never hooks or configuration.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn publish_committed(
    git: &HostGit,
    source: &str,
    source_lfs: &str,
    source_ref: &str,
    expected_commit: &str,
    branch: &str,
    remote: &str,
    token: Option<&str>,
) -> Result<u64, String> {
    let mut imported = false;
    publish_committed_tracking(
        git,
        source,
        source_lfs,
        source_ref,
        expected_commit,
        branch,
        remote,
        token,
        &mut imported,
    )
}
/// `imported` becomes true once the computer commit is fully in the cache;
/// later failures (remote rejections, network) leave the cache consistent.
#[allow(clippy::too_many_arguments)]
fn publish_committed_tracking(
    git: &HostGit,
    source: &str,
    source_lfs: &str,
    source_ref: &str,
    expected_commit: &str,
    branch: &str,
    remote: &str,
    token: Option<&str>,
    imported_into_cache: &mut bool,
) -> Result<u64, String> {
    git.run(&["check-ref-format", "--branch", branch], None, "")?;
    git.run(&["init", "--bare", "--quiet"], None, "")?;
    // This host-owned cache contains only Silo's configuration. Repoint both
    // remotes for every invocation; no guest configuration crosses this seam.
    git.run(
        &["config", "--replace-all", "remote.origin.url", remote],
        None,
        "",
    )?;
    git.run(
        &["config", "--replace-all", "remote.silo-source.url", source],
        None,
        "",
    )?;
    git.run(
        &[
            "fetch",
            "--no-tags",
            "silo-source",
            &format!("+{source_ref}:refs/silo/push"),
        ],
        None,
        "",
    )?;
    let imported = git.run(&["rev-parse", "refs/silo/push"], None, "")?;
    if imported.trim() != expected_commit {
        return Err("The computer repository changed during export. Retry the push.".into());
    }
    *imported_into_cache = true;
    // fetch.fsckObjects verifies incoming objects without rescanning the
    // complete trusted cache on every incremental push.
    // Only the currently advertised destination ref may exclude LFS uploads.
    // A previous cached branch must not suppress objects for a new destination.
    for stale in ["refs/remotes/origin/published", "refs/silo/remote-base"] {
        git.run(&["update-ref", "-d", stale], None, "")?;
    }
    let target_ref = format!("refs/heads/{branch}");
    let remote_head = git.run(
        &["ls-remote", "--heads", "origin", &target_ref],
        token,
        remote,
    )?;
    let range = if remote_head.trim().is_empty() {
        // A new branch: count only commits missing from the remote's default
        // branch, not the whole history. An empty repository has no default
        // branch, so every commit is new. Having that branch locally also
        // keeps Git from re-sending history GitHub already has.
        match git.run(
            &[
                "fetch",
                "--no-tags",
                "origin",
                "+HEAD:refs/silo/remote-base",
            ],
            token,
            remote,
        ) {
            Ok(_) => "refs/silo/remote-base..refs/silo/push",
            Err(error) if matches!(error.lines().next(), Some(CANCELLED | CREDENTIAL_EXPIRED)) => {
                return Err(error)
            }
            Err(_) => "refs/silo/push",
        }
    } else {
        git.run(
            &[
                "fetch",
                "--no-tags",
                "origin",
                &format!("+{target_ref}:refs/remotes/origin/published"),
            ],
            token,
            remote,
        )?;
        // Ask Git before spending time transferring LFS data. The final push
        // still performs Git's own concurrent-update/non-fast-forward checks.
        git.run(&["merge-base", "--is-ancestor", "refs/remotes/origin/published", "refs/silo/push"], None, "")
            .map_err(|_| "The remote branch has commits missing from this computer. Fetch and integrate them before pushing.".to_string())?;
        "refs/remotes/origin/published..refs/silo/push"
    };
    let count = git
        .run(&["rev-list", "--count", range], None, "")?
        .trim()
        .parse()
        .map_err(|_| "Invalid commit count.")?;

    // --all includes LFS data referenced only by historical commits. An object
    // absent from the computer may already exist upstream. LFS itself decides
    // whether such an object is needed; a source fetch alone is not the gate.
    let source_result = git.run(
        &[
            "-c",
            &format!("lfs.url={source_lfs}"),
            "-c",
            "lfs.sshtransfer=always",
            "-c",
            "lfs.ssh.variant=ssh",
            "lfs",
            "fetch",
            "--all",
            "silo-source",
            "refs/silo/push",
        ],
        None,
        "",
    );
    if let Err(first_push) = git.run(&["lfs", "push", "origin", "refs/silo/push"], token, remote) {
        if matches!(
            first_push.lines().next(),
            Some(CANCELLED | CREDENTIAL_EXPIRED)
        ) {
            return Err(first_push);
        }
        // Standard LFS fetch fills pruned historical data from the destination.
        // Content-addressed LFS uploads can safely be retried before any Git ref
        // update. Never enable allowincompletepush or parse a human transfer plan.
        let upstream_result = git.run(
            &["lfs", "fetch", "--all", "origin", "refs/silo/push"],
            token,
            remote,
        );
        git.run(&["lfs", "push", "origin", "refs/silo/push"], token, remote)
            .map_err(|error| {
                format!(
                    "{error} Source transfer: {} Upstream recovery: {} First upload: {first_push}",
                    source_result.err().unwrap_or_else(|| "completed".into()),
                    upstream_result.err().unwrap_or_else(|| "completed".into())
                )
            })?;
    }
    git.run(
        &[
            "push",
            "--porcelain",
            "origin",
            &format!("refs/silo/push:{target_ref}"),
        ],
        token,
        remote,
    )
    .map_err(final_push_error)?;
    Ok(count)
}

// The opt-in live regression pushes the computer's current branch, as the UI
// would after the user confirmed it.
#[cfg(test)]
pub(crate) fn push_committed(
    paths: &RuntimePaths,
    computer: &str,
    path: &str,
    repo: &str,
    token: &str,
    executable: &Path,
    support: &Path,
) -> Result<u64, String> {
    let metadata = runtime::read_metadata(&paths.metadata).map_err(|e| e.to_string())?;
    let computer_id = metadata
        .computers
        .iter()
        .find(|m| m.name() == computer)
        .map(|m| m.id().to_owned())
        .ok_or("Choose a managed Silo computer.")?;
    let head = guest(
        paths,
        computer,
        "set -eu\nprintf '%s\\n' \"$(git -C \"$1\" symbolic-ref --quiet --short HEAD)\" \"$(git -C \"$1\" rev-parse --verify HEAD)\"",
        &[path],
    )?;
    let mut lines = head.lines();
    let target = PushTarget {
        repository: repo.into(),
        branch: lines.next().unwrap_or_default().into(),
        commit: lines.next().unwrap_or_default().into(),
    };
    target.validate()?;
    push_target(
        paths,
        computer,
        &computer_id,
        path,
        &target,
        repo,
        token,
        None,
        executable,
        support,
    )
}

// The same publication path is exercised with disposable computers and scoped
// credentials in the opt-in live regression. Authorization stays in perform.
#[allow(clippy::too_many_arguments)]
fn push_target(
    paths: &RuntimePaths,
    computer: &str,
    computer_id: &str,
    path: &str,
    target: &PushTarget,
    repo: &str,
    token: &str,
    deadline: Option<Instant>,
    executable: &Path,
    support: &Path,
) -> Result<u64, String> {
    let id = uuid::Uuid::new_v4();
    let export = format!("/tmp/silo-push-{id}");
    let export_ref = format!("refs/silo/export/{id}");
    let (branch, commit) = (target.branch.as_str(), target.commit.as_str());
    let result = (|| {
        let temp = tempfile::tempdir().map_err(|_| "Cannot create isolated host Git directory.")?;
        let root = temp.path();
        let cache_key = format!("{computer}\0{path}\0{repo}");
        let cache = crate::host_push_cache::acquire(&paths.home.join("push-cache"), &cache_key)?;
        let mut transport =
            crate::host_push_transport::prepare(paths, computer, &root.join("ssh"))?;
        transport.install_lfs_server(&support.join("lfs-transfer/git-lfs-transfer"), &export)?;
        // Export only the confirmed branch, and only while it still points at
        // the confirmed commit. The host verifies the imported commit again.
        let data = guest(
            paths,
            computer,
            r#"set -eu
commit=$(git -C "$1" rev-parse --verify --quiet "refs/heads/$4^{commit}") || commit=
if [ "$commit" != "$5" ]; then printf 'changed\n'; exit 0; fi
tracking=$(git -C "$1" rev-parse --verify --quiet "refs/remotes/origin/$4") || tracking=
origin=$(git -C "$1" remote get-url origin)
git -C "$1" update-ref "$3" "$commit"
# Use Git LFS's own storage resolution, including linked worktrees and lfs.storage.
media=$(git -C "$1" lfs env | sed -n 's/^LocalMediaDir=//p')
case "$media" in /*) ;; *) echo 'Git LFS returned no absolute media directory' >&2; exit 1;; esac
mkdir -p "$2/source.git/lfs"
if [ -d "$media" ]; then
    ln -s -- "$media" "$2/source.git/lfs/objects"
else
    mkdir "$2/source.git/lfs/objects"
fi
printf '%s\n%s\n%s\n' "$commit" "$tracking" "$origin"
"#,
            &[path, &export, &export_ref, branch, commit],
        )?;
        let mut data = data.lines();
        if data.next() != Some(commit) {
            return Err(TARGET_CHANGED.into());
        }
        let expected_tracking = data.next().unwrap_or_default();
        let expected_origin = data.next().unwrap_or_default();
        if !repository(expected_origin)
            .is_ok_and(|repository| repository.eq_ignore_ascii_case(&target.repository))
        {
            return Err(TARGET_CHANGED.into());
        }
        let git = HostGit {
            executable: executable.to_path_buf(),
            directory: cache.directory.join("repository.git"),
            home: root.join("home"),
            support: support.to_path_buf(),
            ssh_command: Some(transport.ssh_command.clone()),
            cache_lock_fd: Some(cache.lock_fd()),
            deadline,
        };
        fs::create_dir_all(&git.directory)
            .and_then(|_| fs::create_dir_all(git.home.join("empty-templates")))
            .map_err(|_| "Cannot create isolated host Git directory.")?;
        let source = transport.repository_url(path)?;
        let source_lfs = format!("ssh://{}{export}/source.git", transport.alias);
        let mut imported = false;
        let publication = publish_committed_tracking(
            &git,
            &source,
            &source_lfs,
            &export_ref,
            commit,
            branch,
            &format!("https://github.com/{repo}.git"),
            Some(token),
            &mut imported,
        );
        // Keep a consistent cache across remote rejections and network
        // failures so large (LFS) repositories do not re-transfer on retry.
        if publication.is_err() && !imported {
            cache.discard();
        }
        let count = publication?;
        // Record the published commit only if the guest has not fetched newer
        // tracking data or repointed origin while publication was running.
        let _ = runtime::operation_gate::uncancellable(|| {
            guest(
                paths,
                computer,
                UPDATE_TRACKING_REF,
                &[
                    path,
                    &format!("refs/remotes/origin/{branch}"),
                    commit,
                    expected_tracking,
                    expected_origin,
                ],
            )
        });
        // The next state refresh reads the repository again.
        if let Ok(mut entries) = discoveries().0.lock() {
            if let Some(entry) = entries.get_mut(&format!("{}:{computer_id}", paths.home.display()))
            {
                entry.invalidate();
            }
        }
        Ok(count)
    })();
    // Clean up the export even after a cancel.
    let _ = runtime::operation_gate::uncancellable(|| {
        guest(
            paths,
            computer,
            "git -C \"$1\" update-ref -d \"$3\"; rm -rf -- \"$2\"",
            &[path, &export, &export_ref],
        )
    });
    result
}
/// Push a local computer repository. Remote devices run this through their
/// own push journal (`repository.push.start`).
pub(crate) async fn push_repository(
    app: tauri::AppHandle,
    computer: String,
    repository_path: String,
    target: PushTarget,
) -> Result<Value, String> {
    let key = format!("{computer}\0{repository_path}");
    let planned_count = {
        let (app, computer, repository_path) =
            (app.clone(), computer.clone(), repository_path.clone());
        runtime::operation_gate::spawn_blocking(move || {
            planned_count(&app, &computer, &repository_path)
        })
        .await
        .map_err(|_| "Host push task failed.".to_string())?
    };
    {
        let mut r = results().lock().map_err(|_| "Push state unavailable.")?;
        if r.get(&key).is_some_and(|(v, _)| v["status"] == "pushing") {
            return Err("This repository is already being pushed.".into());
        }
        r.insert(key.clone(),(json!({"computer":computer,"repositoryPath":repository_path,"commitCount":planned_count,"status":"pushing","target":target}),Instant::now()));
    }
    let _ = app.emit("silo://application-state-changed", ());
    let task = {
        let (app, computer, repository_path, target) = (
            app.clone(),
            computer.clone(),
            repository_path.clone(),
            target.clone(),
        );
        runtime::operation_gate::spawn_blocking(move || {
            perform(&app, &computer, &repository_path, &target)
        })
    };
    // A panicked task must still resolve the entry, or it would stay
    // "pushing" forever and block every retry.
    let outcome = task
        .await
        .unwrap_or_else(|_| Err("Host push task failed.".into()));
    let value = finished_result(&computer, &repository_path, &target, outcome);
    results()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(key, (value.clone(), Instant::now()));
    let _ = app.emit("silo://application-state-changed", ());
    Ok(value)
}
/// The commit count last shown for this repository, so an active push reports
/// the planned number instead of zero.
pub(crate) fn planned_count(app: &tauri::AppHandle, computer: &str, repository_path: &str) -> u64 {
    runtime::runtime_paths(app)
        .ok()
        .and_then(|paths| {
            let metadata = runtime::read_metadata(&paths.metadata).ok()?;
            let computer_id = metadata
                .computers
                .iter()
                .find(|configuration| configuration.name() == computer)?
                .id();
            discoveries()
                .0
                .lock()
                .ok()?
                .get(&format!("{}:{computer_id}", paths.home.display()))?
                .last
                .as_ref()?
                .1
                .as_ref()
                .ok()?
                .iter()
                .find(|repo| repo["path"] == repository_path)?["ahead"]
                .as_u64()
        })
        .unwrap_or(0)
}
fn finished_result(
    computer: &str,
    repository_path: &str,
    target: &PushTarget,
    outcome: Result<u64, String>,
) -> Value {
    match outcome {
        Ok(count) => json!({
            "computer": computer,
            "repositoryPath": repository_path,
            "commitCount": count,
            "status": "succeeded",
            "target": target,
        }),
        Err(message) if message == PUBLICATION_UNKNOWN => json!({
            "computer": computer,
            "repositoryPath": repository_path,
            "commitCount": 0,
            "status": "unknown",
            "message": message,
            "target": target,
        }),
        Err(message) => {
            let mut value = json!({
                "computer": computer,
                "repositoryPath": repository_path,
                "commitCount": 0,
                "status": "failed",
                "target": target,
            });
            // Keep the visible message short; Git output goes to the Details disclosure.
            match message.split_once('\n') {
                Some((summary, details)) if !details.trim().is_empty() => {
                    value["message"] = json!(summary.trim());
                    value["diagnosticDetails"] = json!(details.trim());
                }
                _ => value["message"] = json!(message.trim()),
            }
            value
        }
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn a_panicking_discovery_reports_an_error() {
        let result = super::contain_panic(|| panic!("discovery bug"));
        assert!(result.is_err());
        assert_eq!(super::contain_panic(|| Ok(Vec::new())), Ok(Vec::new()));
    }

    #[test]
    fn dismissal_removes_finished_results_but_preserves_active_pushes() {
        let mut entries = std::collections::HashMap::new();
        for status in ["failed", "succeeded", "pushing"] {
            entries.insert(
                status.into(),
                (
                    serde_json::json!({"status":status}),
                    std::time::Instant::now(),
                ),
            );
        }
        for status in ["failed", "succeeded", "pushing"] {
            super::dismiss_result(&mut entries, status);
        }
        assert_eq!(entries.len(), 1);
        assert!(entries.contains_key("pushing"));
        super::dismiss_result(&mut entries, "failed");
        assert_eq!(entries.len(), 1);
    }

    use super::*;
    #[test]
    fn recreated_computer_cannot_receive_the_previous_computers_repository_discovery() {
        let root = tempfile::tempdir().unwrap();
        let paths = RuntimePaths {
            executable: root.path().join("missing-msb"),
            home: root.path().to_path_buf(),
            guest_image: root.path().join("image"),
            storage_home: None,
            library: root.path().join("library"),
            metadata: root.path().join("metadata"),
            volumes: root.path().join("volumes"),
        };
        let key = format!("{}:computer-old", paths.home.display());
        let cached = vec![json!({"path": "previous-computer-private-repository"})];
        discoveries().0.lock().unwrap().insert(
            key.clone(),
            Discovery {
                last: Some((Instant::now(), Ok(cached.clone()))),
                running: false,
                generation: 0,
                stale: false,
                served: None,
            },
        );
        assert_eq!(
            discover(&paths, "dev", "computer-old", false).unwrap(),
            cached
        );
        let replacement = discover(&paths, "dev", "computer-new", false);
        discoveries().0.lock().unwrap().remove(&key);
        discoveries()
            .0
            .lock()
            .unwrap()
            .remove(&format!("{}:computer-new", paths.home.display()));
        assert!(replacement.is_err(), "The replacement computer must discover its own repositories instead of returning the previous computer's cached rows: {replacement:?}");
    }

    #[test]
    fn manual_discovery_bypasses_cached_rows() {
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("missing-msb");
        let paths = RuntimePaths {
            executable,
            home: root.path().to_path_buf(),
            guest_image: root.path().join("image"),
            storage_home: None,
            library: root.path().join("library"),
            metadata: root.path().join("metadata"),
            volumes: root.path().join("volumes"),
        };
        let key = format!("{}:computer-1", paths.home.display());
        let cached = vec![json!({"path": "removed-repository"})];
        discoveries().0.lock().unwrap().insert(
            key.clone(),
            Discovery {
                last: Some((Instant::now(), Ok(cached.clone()))),
                running: false,
                generation: 0,
                stale: false,
                served: None,
            },
        );
        assert_eq!(
            discover(&paths, "test", "computer-1", false).unwrap(),
            cached
        );
        // A forced read must reach the missing runtime instead of returning
        // the fresh cached rows. No real computer or runtime is involved.
        let refreshed = discover(&paths, "test", "computer-1", true);
        assert!(refreshed.is_err());
        assert_eq!(discover(&paths, "test", "computer-1", false), refreshed);
        discoveries().0.lock().unwrap().remove(&key);
    }

    #[test]
    fn discovery_keeps_repositories_beyond_two_hundred_entries() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let seed = root.path().join("seed");
        fs::create_dir(&seed).unwrap();
        for args in [
            vec!["init", "--quiet", "--initial-branch=main"],
            vec![
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "--quiet",
                "--allow-empty",
                "-m",
                "fixture",
            ],
            vec![
                "remote",
                "add",
                "origin",
                "https://github.com/Owner/Repo.git",
            ],
        ] {
            assert!(Command::new("git")
                .args(args)
                .current_dir(&seed)
                .status()
                .unwrap()
                .success());
        }
        let head = String::from_utf8(
            Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&seed)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        let computer = root.path().join("computer with spaces");
        fs::create_dir(&computer).unwrap();
        for index in 0..216 {
            let repo = computer.join(format!("repo-{index}"));
            fs::create_dir(&repo).unwrap();
            symlink(seed.join(".git"), repo.join(".git")).unwrap();
        }
        let output = Command::new("sh")
            .args(["-c", DISCOVER_REPOSITORIES, "silo-host-push"])
            .arg(&computer)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "scan exited with {}",
            output.status
        );
        let output = String::from_utf8(output.stdout).unwrap();
        let records: Vec<_> = output.split('\0').collect();
        assert_eq!(records.len(), 216 * DISCOVERY_FIELDS + 1);
        for record in records.chunks_exact(DISCOVERY_FIELDS) {
            assert_eq!(record[1], "main");
            assert_eq!(record[2], "1 0");
            assert_eq!(record[3], "");
        }
        // The computer root is outside /workspace here; rename it for parsing.
        let rows = discovered_rows(&output.replace(computer.to_str().unwrap(), "/workspace"));
        assert_eq!(rows.len(), 216);
        assert_eq!(rows[0]["repository"], "Owner/Repo");
        assert_eq!(rows[0]["head"], head.trim());
    }

    #[test]
    fn discovery_started_before_a_push_cannot_restore_stale_rows() {
        let mut entry = Discovery {
            running: true,
            ..Discovery::default()
        };
        let generation = entry.generation;
        let started = Instant::now();
        entry.invalidate();
        entry.finish(
            generation,
            started,
            Ok(vec![json!({"path":"/workspace/repo","ahead":1})]),
        );
        assert!(
            entry.stale && entry.last.is_none(),
            "the pre-push discovery restored stale rows"
        );
        assert!(
            !entry.running,
            "the next refresh must be able to start a read"
        );
        entry.running = true;
        entry.finish(
            entry.generation,
            Instant::now(),
            Ok(vec![json!({"path":"/workspace/repo","ahead":0})]),
        );
        assert_eq!(entry.last.unwrap().1.unwrap()[0]["ahead"], 0);
    }

    /// A runtime whose guest runs the shell command `wait` during each discovery and
    /// counts them.
    fn slow_discovery_runtime(root: &Path, wait: &str) -> (RuntimePaths, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let paths = RuntimePaths {
            executable: root.join("msb"),
            home: root.join("home"),
            guest_image: root.join("image"),
            storage_home: None,
            // The runtime checks that its library exists; the script stands in.
            library: root.join("msb"),
            metadata: root.join("computers.json"),
            volumes: root.join("volumes"),
        };
        let count = root.join("discoveries");
        let inspected =
            json!({"name":"dev","status":"Running","config":{"labels":{"silo.managed":"true"}}});
        fs::write(
            &paths.executable,
            format!(
                "#!/bin/sh\ncase \"$1\" in\n--silo-working-account-protocol) printf '1\\n' ;;\ninspect) printf '%s\\n' '{inspected}' ;;\nexec) echo run >>'{}'; {wait}; printf '/workspace/repo\\0main\\0001 0\\0\\0{}\\0https://github.com/owner/repo.git\\0' ;;\n*) exit 2 ;;\nesac\n",
                count.display(),
                "e".repeat(40),
            ),
        )
        .unwrap();
        fs::set_permissions(&paths.executable, fs::Permissions::from_mode(0o700)).unwrap();
        (paths, count)
    }
    fn runs(count: &Path) -> usize {
        fs::read_to_string(count).map_or(0, |text| text.lines().count())
    }

    #[test]
    fn discovery_reads_each_computer_once_in_the_background_and_serves_known_rows() {
        let root = tempfile::tempdir().unwrap();
        // The guest read blocks until the test opens the gate, so no step depends on timing.
        let gate = root.path().join("gate");
        let wait = format!("while [ ! -e '{}' ]; do sleep 0.02; done", gate.display());
        let (paths, count) = slow_discovery_runtime(root.path(), &wait);
        let wait_until = |what: &str, done: &dyn Fn() -> bool| {
            let deadline = Instant::now() + Duration::from_secs(60);
            while !done() {
                assert!(Instant::now() < deadline, "{what}");
                thread::sleep(Duration::from_millis(10));
            }
        };
        // Concurrent state refreshes share one guest read.
        let readers: Vec<_> = (0..3)
            .map(|_| {
                let paths = paths.clone();
                thread::spawn(move || discover(&paths, "dev", "computer-1", false))
            })
            .collect();
        wait_until("the guest read never started", &|| runs(&count) >= 1);
        fs::write(&gate, b"open").unwrap();
        for reader in readers {
            let rows = reader.join().unwrap().unwrap();
            assert_eq!(rows[0]["repository"], "owner/repo");
        }
        assert_eq!(runs(&count), 1);
        // Once stale, the known rows are returned at once while a new read runs.
        let key = format!("{}:computer-1", paths.home.display());
        discoveries()
            .0
            .lock()
            .unwrap()
            .get_mut(&key)
            .unwrap()
            .last
            .as_mut()
            .unwrap()
            .0 = Instant::now() - Duration::from_secs(60);
        fs::remove_file(&gate).unwrap();
        assert_eq!(
            discover(&paths, "dev", "computer-1", false).unwrap().len(),
            1
        );
        // The call returned while the new read is still blocked in the guest: it served
        // the known rows instead of waiting for it.
        wait_until("background discovery never started", &|| runs(&count) >= 2);
        assert!(discoveries().0.lock().unwrap()[&key].running);
        fs::write(&gate, b"open").unwrap();
        wait_until("background discovery did not finish", &|| {
            !discoveries().0.lock().unwrap()[&key].running
        });
        assert_eq!(runs(&count), 2);
        discoveries().0.lock().unwrap().remove(&key);
    }

    #[test]
    fn a_change_keeps_known_rows_as_a_fallback_while_the_next_read_runs() {
        let root = tempfile::tempdir().unwrap();
        let (paths, _count) = slow_discovery_runtime(root.path(), "sleep 6");
        let key = format!("{}:computer-stale", paths.home.display());
        let known = Ok(vec![json!({"path":"/workspace/repo","ahead":3})]);
        {
            let mut entries = discoveries().0.lock().unwrap();
            let entry = entries.entry(key.clone()).or_default();
            entry.running = true;
            entry.finish(entry.generation, Instant::now(), known);
            entry.invalidate();
        }
        let started = Instant::now();
        let rows = discover_until(
            &paths,
            "dev",
            "computer-stale",
            false,
            Instant::now() + Duration::from_millis(300),
        )
        .unwrap();
        assert_eq!(rows[0]["ahead"], 3);
        assert!(started.elapsed() < Duration::from_secs(3));
        discoveries().0.lock().unwrap().remove(&key);
    }

    #[test]
    fn a_finished_read_is_announced_only_when_it_differs_from_what_was_served() {
        let rows = |ahead: u64| Ok(vec![json!({"path":"/workspace/repo","ahead":ahead})]);
        let mut entry = Discovery {
            running: true,
            served: Some(rows(1)),
            ..Discovery::default()
        };
        entry.finish(entry.generation, Instant::now(), rows(0));
        assert!(entry.differs_from_served());
        entry.running = true;
        entry.served = Some(rows(0));
        entry.finish(entry.generation, Instant::now(), rows(0));
        assert!(!entry.differs_from_served());
        entry.running = true;
        entry.finish(entry.generation, Instant::now(), rows(5));
        assert!(!entry.differs_from_served(), "nothing was served meanwhile");
    }

    #[test]
    fn computers_share_one_first_discovery_wait() {
        let root = tempfile::tempdir().unwrap();
        let (paths, _count) = slow_discovery_runtime(root.path(), "sleep 6");
        let ids = ["shared-1", "shared-2", "shared-3"];
        prefetch(&paths, ids.iter().map(|id| ("dev", *id)));
        let deadline = Instant::now() + Duration::from_millis(600);
        let started = Instant::now();
        for id in ids {
            assert!(discover_until(&paths, "dev", id, false, deadline)
                .unwrap()
                .is_empty());
        }
        assert!(started.elapsed() < Duration::from_secs(2));
        for id in ids {
            discoveries()
                .0
                .lock()
                .unwrap()
                .remove(&format!("{}:{id}", paths.home.display()));
        }
    }

    #[test]
    fn a_slow_guest_does_not_stall_state_refreshes() {
        let root = tempfile::tempdir().unwrap();
        let (paths, count) = slow_discovery_runtime(root.path(), "sleep 6");
        let started = Instant::now();
        assert!(discover(&paths, "dev", "computer-1", false)
            .unwrap()
            .is_empty());
        assert!(started.elapsed() < DISCOVERY_FIRST_WAIT + Duration::from_secs(1));
        // Later refreshes do not start another read or wait for this one.
        let started = Instant::now();
        assert!(discover(&paths, "dev", "computer-1", false)
            .unwrap()
            .is_empty());
        assert!(started.elapsed() < DISCOVERY_FIRST_WAIT + Duration::from_secs(1));
        assert_eq!(runs(&count), 1);
        // An explicit refresh waits for the read to finish.
        assert_eq!(
            discover(&paths, "dev", "computer-1", true).unwrap().len(),
            1
        );
        discoveries()
            .0
            .lock()
            .unwrap()
            .remove(&format!("{}:computer-1", paths.home.display()));
    }

    #[test]
    fn discovery_rejects_unreadable_worktree_status() {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("repository");
        fs::create_dir(&repository).unwrap();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .args([
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.invalid",
                ])
                .args(args)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .current_dir(&repository)
                .output()
                .unwrap();
            assert!(output.status.success(), "{args:?}: {:?}", output.stderr);
            String::from_utf8(output.stdout).unwrap()
        };
        git(&["init", "--quiet", "--initial-branch=main"]);
        fs::write(repository.join("README"), "committed\n").unwrap();
        git(&["add", "README"]);
        git(&["commit", "--quiet", "-m", "fixture"]);
        git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
        fs::write(repository.join("README"), "uncommitted\n").unwrap();
        let discover = || {
            Command::new("/bin/sh")
                .args(["-c", DISCOVER_REPOSITORIES, "silo-host-push"])
                .arg(root.path())
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .unwrap()
        };
        let healthy = discover();
        assert!(healthy.status.success());
        let record = String::from_utf8(healthy.stdout).unwrap();
        assert!(record.split('\0').nth(3).unwrap().contains("README"));
        fs::write(repository.join(".git/index"), "corrupt index").unwrap();
        let output = discover();
        assert!(
            !output.status.success(),
            "A failed status read must not publish a clean repository: {:?}",
            output.stdout
        );
    }

    #[test]
    fn discovery_counts_only_unpublished_commits_of_a_new_branch() {
        let root = tempfile::tempdir().unwrap();
        let computer = root.path().join("computer");
        let repository = computer.join("repo");
        fs::create_dir_all(&repository).unwrap();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .args([
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.invalid",
                ])
                .args(args)
                .current_dir(&repository)
                .output()
                .unwrap();
            assert!(output.status.success(), "{args:?}");
            String::from_utf8(output.stdout).unwrap()
        };
        git(&["init", "--quiet", "--initial-branch=main"]);
        for message in ["one", "two"] {
            git(&["commit", "--quiet", "--allow-empty", "-m", message]);
        }
        let published = git(&["rev-parse", "HEAD"]);
        git(&["update-ref", "refs/remotes/origin/main", published.trim()]);
        git(&["switch", "--quiet", "-c", "feature"]);
        git(&["commit", "--quiet", "--allow-empty", "-m", "three"]);
        let output = Command::new("sh")
            .args(["-c", DISCOVER_REPOSITORIES, "silo-host-push"])
            .arg(&computer)
            .output()
            .unwrap();
        let output = String::from_utf8(output.stdout).unwrap();
        let record: Vec<_> = output.split('\0').collect();
        assert_eq!(record[1], "feature");
        // Not the whole history (3): only the commit missing from origin.
        assert_eq!(record[2], "1 0");
    }

    #[test]
    fn discovery_skips_dependency_trees() {
        let root = tempfile::tempdir().unwrap();
        let computer = root.path().join("computer");
        for repository in [
            "app",
            "app/node_modules/dependency",
            "tool/.venv/lib/package",
        ] {
            let directory = computer.join(repository);
            fs::create_dir_all(&directory).unwrap();
            assert!(Command::new("git")
                .args(["init", "--quiet", "--initial-branch=main"])
                .current_dir(&directory)
                .status()
                .unwrap()
                .success());
        }
        let output = Command::new("sh")
            .args(["-c", DISCOVER_REPOSITORIES, "silo-host-push"])
            .arg(&computer)
            .output()
            .unwrap();
        let output = String::from_utf8(output.stdout).unwrap();
        let paths: Vec<_> = output
            .split('\0')
            .step_by(DISCOVERY_FIELDS)
            .filter(|path| !path.is_empty())
            .collect();
        assert_eq!(paths, [computer.join("app").to_str().unwrap()]);
    }

    #[test]
    fn discovered_rows_omit_unverifiable_push_destinations() {
        let row = |head: &str, origin: &str| {
            format!("/workspace/repo\0main\x001 0\0\0{head}\0{origin}\0")
        };
        let commit = "a".repeat(40);
        let rows = discovered_rows(&row(&commit, "git@github.com:owner/repo.git"));
        assert_eq!(rows[0]["repository"], "owner/repo");
        assert_eq!(rows[0]["head"], commit);
        let rows = discovered_rows(&row("not-a-commit", "https://gitlab.com/owner/repo.git"));
        assert!(rows[0]["repository"].is_null());
        assert!(rows[0]["head"].is_null());
    }

    #[test]
    fn tracking_metadata_preserves_concurrent_fetches_and_origin_changes() {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("repo");
        fs::create_dir(&repository).unwrap();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .args([
                    "-c",
                    "core.hooksPath=/dev/null",
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.invalid",
                ])
                .args(args)
                .env("HOME", root.path())
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .current_dir(&repository)
                .output()
                .unwrap();
            assert!(output.status.success(), "{args:?}");
            String::from_utf8(output.stdout).unwrap().trim().to_owned()
        };
        git(&["init", "--quiet", "--initial-branch=main"]);
        let commits: Vec<_> = ["before", "published", "fetched"]
            .into_iter()
            .map(|message| {
                git(&["commit", "--quiet", "--allow-empty", "-m", message]);
                git(&["rev-parse", "HEAD"])
            })
            .collect();
        let origin = "https://github.com/owner/repo.git";
        let tracking = "refs/remotes/origin/main";
        git(&["remote", "add", "origin", origin]);
        let update = |expected: &str| {
            Command::new("sh")
                .args(["-c", UPDATE_TRACKING_REF, "silo-host-push"])
                .arg(&repository)
                .args([tracking, &commits[1], expected, origin])
                .env("HOME", root.path())
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .unwrap()
                .status
        };
        git(&["update-ref", tracking, &commits[2]]);
        let _ = update(&commits[0]);
        assert_eq!(git(&["rev-parse", tracking]), commits[2]);
        git(&["update-ref", tracking, &commits[0]]);
        git(&[
            "remote",
            "set-url",
            "origin",
            "https://github.com/another/repo.git",
        ]);
        assert!(update(&commits[0]).success());
        assert_eq!(git(&["rev-parse", tracking]), commits[0]);
        git(&["remote", "set-url", "origin", origin]);
        assert!(update(&commits[0]).success());
        assert_eq!(git(&["rev-parse", tracking]), commits[1]);
        git(&["update-ref", "-d", tracking]);
        assert!(update("").success());
        assert_eq!(git(&["rev-parse", tracking]), commits[1]);
        git(&["update-ref", "refs/heads/main", &commits[1]]);
        git(&["update-ref", "refs/heads/work", &commits[2]]);
        git(&["symbolic-ref", tracking, "refs/heads/work"]);
        assert!(update(&commits[2]).success());
        assert_eq!(git(&["rev-parse", "refs/heads/work"]), commits[2]);
        assert_eq!(git(&["rev-parse", tracking]), commits[1]);
    }

    #[test]
    fn push_targets_are_validated_before_any_work() {
        let target = PushTarget {
            repository: "owner/repo".into(),
            branch: "feature/x".into(),
            commit: "b".repeat(40),
        };
        assert!(target.validate().is_ok());
        for invalid in [
            PushTarget {
                repository: "owner/repo/extra".into(),
                ..target.clone()
            },
            PushTarget {
                branch: "".into(),
                ..target.clone()
            },
            PushTarget {
                branch: "-delete".into(),
                ..target.clone()
            },
            PushTarget {
                branch: "main\nother".into(),
                ..target.clone()
            },
            PushTarget {
                commit: "HEAD".into(),
                ..target.clone()
            },
        ] {
            assert!(invalid.validate().is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn rejects_untrusted_remote_destinations() {
        for url in [
            "https://github.com.evil/a/b",
            "https://user@github.com/a/b",
            "https://github.com/a/b?token=x",
            "https://github.com/a/../b",
            "ssh://evil/a/b",
            "https://github.com/a/b/c",
        ] {
            assert!(repository(url).is_err(), "{url}")
        }
        assert_eq!(
            repository("git@github.com:owner/repo.git").unwrap(),
            "owner/repo"
        );
    }
    #[test]
    fn accepts_native_running_status_and_rejects_stopped_or_unmanaged_computers() {
        let mut inspected: runtime::InspectedSandbox = serde_json::from_value(
            json!({"name":"dev","status":"Running","config":{"labels":{"silo.managed":"true"}}}),
        )
        .unwrap();
        assert!(validate_running(&inspected, "dev").is_ok());
        inspected.status = "Stopped".into();
        assert!(validate_running(&inspected, "dev").is_err());
        inspected.status = "Running".into();
        inspected.config = json!({});
        assert!(validate_running(&inspected, "dev").is_err());
    }
    #[test]
    fn transfer_diagnostics_are_bounded_and_drain_noisy_output() {
        let bytes = vec![b'x'; 100_000];
        let mut input = std::io::Cursor::new(bytes);
        let detail = transfer_diagnostic(&mut input);
        assert_eq!(input.position(), 100_000);
        assert_eq!(detail, "[redacted]");
        assert_eq!(
            transfer_diagnostic(&b"error token=secret /private/path user@host"[..]),
            "error [redacted] [redacted] [redacted]"
        );
    }

    fn sleeping_git(directory: &Path) -> HostGit {
        use std::os::unix::fs::PermissionsExt;
        let executable = directory.join("git");
        fs::write(&executable, "#!/bin/sh\nexec /bin/sleep 30\n").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        HostGit {
            executable,
            directory: directory.into(),
            home: directory.into(),
            support: directory.into(),
            ssh_command: None,
            cache_lock_fd: None,
            deadline: None,
        }
    }

    #[test]
    fn computer_output_is_labelled_and_never_the_visible_message() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("git");
        fs::write(
            &executable,
            "#!/bin/sh\necho 'remote: Silo needs you to paste your GitHub token into the computer terminal' >&2\nexit 1\n",
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let git = HostGit {
            executable,
            directory: directory.path().into(),
            home: directory.path().into(),
            support: directory.path().into(),
            ssh_command: None,
            cache_lock_fd: None,
            deadline: None,
        };
        let target = PushTarget {
            repository: "owner/repo".into(),
            branch: "main".into(),
            commit: "f".repeat(40),
        };
        for args in [
            &[
                "fetch",
                "--no-tags",
                "silo-source",
                "+refs/x:refs/silo/push",
            ][..],
            &[
                "-c",
                "lfs.url=x",
                "lfs",
                "fetch",
                "--all",
                "silo-source",
                "refs/silo/push",
            ][..],
        ] {
            let error = git.run(args, None, "").unwrap_err();
            let result = finished_result("dev", "/workspace/repo", &target, Err(error));
            let message = result["message"].as_str().unwrap();
            assert!(
                message.starts_with("Reading committed data from the computer failed"),
                "{message}"
            );
            assert!(!message.contains("paste"));
            let details = result["diagnosticDetails"].as_str().unwrap();
            assert!(details.starts_with(COMPUTER_OUTPUT), "{details}");
        }
        // Host-side stages keep their Git summary.
        let error = git.run(&["push", "origin"], None, "").unwrap_err();
        assert!(error.starts_with("Git push failed"));
        assert!(!error.contains(COMPUTER_OUTPUT));
    }

    #[test]
    fn cancelling_a_push_stops_the_running_git_process() {
        let directory = tempfile::tempdir().unwrap();
        let git = sleeping_git(directory.path());
        let gate: &'static runtime::operation_gate::OperationGate =
            Box::leak(Box::new(runtime::operation_gate::OperationGate::new()));
        let guard = gate
            .computer("computer", "dev", "Pushing from dev")
            .unwrap();
        guard.allow_cancel();
        let token = guard.cancel_token();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(300));
            token.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        let started = Instant::now();
        let error = git.run(&["push"], None, "").unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(error.lines().next(), Some(CANCELLED));
    }

    #[test]
    fn a_push_never_outlives_its_credential() {
        let directory = tempfile::tempdir().unwrap();
        let mut git = sleeping_git(directory.path());
        git.deadline = Some(Instant::now() + Duration::from_millis(300));
        let started = Instant::now();
        let error = git.run(&["push"], None, "").unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(error.lines().next(), Some(CREDENTIAL_EXPIRED));
        // A push stopped while GitHub was receiving it has an unknown outcome.
        assert_eq!(final_push_error(error), PUBLICATION_UNKNOWN);
        assert_eq!(
            final_push_error("Git push failed (exit status: 1).\nrejected".into()),
            "Git push failed (exit status: 1).\nrejected"
        );
        let unknown = finished_result(
            "dev",
            "/workspace/repo",
            &PushTarget {
                repository: "owner/repo".into(),
                branch: "main".into(),
                commit: "d".repeat(40),
            },
            Err(PUBLICATION_UNKNOWN.into()),
        );
        assert_eq!(unknown["status"], "unknown");
    }

    #[test]
    fn a_publication_stopped_for_disk_space_has_an_unknown_outcome() {
        let directory = tempfile::tempdir().unwrap();
        let git = sleeping_git(directory.path());
        fs::write(
            &git.executable,
            "#!/bin/sh\nprintf updated >published\nexec /bin/sleep 30\n",
        )
        .unwrap();
        let error = git
            .run_with_budget(&["push"], None, "", |path| {
                if path.join("published").exists() {
                    Err("Cannot check free space for host push.".into())
                } else {
                    Ok(1024 * 1024 * 1024)
                }
            })
            .unwrap_err();
        assert_eq!(
            fs::read_to_string(directory.path().join("published")).unwrap(),
            "updated"
        );
        let target = PushTarget {
            repository: "owner/repo".into(),
            branch: "main".into(),
            commit: "d".repeat(40),
        };
        // Before publication, this interruption remains an ordinary failure.
        let before_push = finished_result("dev", "/workspace/repo", &target, Err(error.clone()));
        assert_eq!(before_push["status"], "failed");
        let result = finished_result(
            "dev",
            "/workspace/repo",
            &target,
            Err(final_push_error(error)),
        );
        assert_eq!(result["status"], "unknown");
        assert_eq!(result["message"], PUBLICATION_UNKNOWN);
    }

    #[test]
    fn publication_failures_require_an_explicit_rejection_to_be_known() {
        let directory = tempfile::tempdir().unwrap();
        let git = sleeping_git(directory.path());
        for (summary, unknown) in [
            ("", true),
            ("[remote failure] (remote failed to report status)", true),
            ("[rejected] (non-fast-forward)", false),
            ("[remote rejected] (hook declined)", false),
        ] {
            let script = if summary.is_empty() {
                "#!/bin/sh\nprintf updated >published\nexit 128\n".to_owned()
            } else {
                format!("#!/bin/sh\nprintf '!\\trefs/silo/push:refs/heads/main\\t%s\\n' '{summary}'\nexit 1\n")
            };
            fs::write(&git.executable, script).unwrap();
            let error = git
                .run(
                    &[
                        "push",
                        "--porcelain",
                        "origin",
                        "refs/silo/push:refs/heads/main",
                    ],
                    None,
                    "",
                )
                .unwrap_err();
            if unknown {
                assert_eq!(error, PUBLICATION_UNKNOWN);
            } else {
                assert!(error.starts_with("Git push failed"), "{error}");
            }
        }
        assert_eq!(
            fs::read_to_string(directory.path().join("published")).unwrap(),
            "updated"
        );
    }

    #[test]
    fn host_git_rejects_oversized_output_without_spooling_to_disk() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("git");
        fs::write(
            &executable,
            "#!/bin/sh\nexec /bin/dd if=/dev/zero bs=65536 count=32\n",
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let git = HostGit {
            executable,
            directory: directory.path().into(),
            home: directory.path().into(),
            support: directory.path().into(),
            ssh_command: None,
            cache_lock_fd: None,
            deadline: None,
        };
        assert!(git.run(&[], None, "").is_err());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }
    #[test]
    fn host_git_passes_the_token_through_a_pipe_not_arguments_or_environment() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("git");
        fs::write(
            &executable,
            "#!/bin/sh\n{ env; printf '%s\\n' \"$@\"; } >observed\nIFS= read -r token <&3 && printf '%s' \"$token\" >credential\n",
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let git = HostGit {
            executable,
            directory: directory.path().into(),
            home: directory.path().into(),
            support: directory.path().into(),
            ssh_command: None,
            cache_lock_fd: None,
            deadline: None,
        };
        let token = "ghu_fixtureToken123";
        git.run(
            &["ls-remote", "origin"],
            Some(token),
            "https://github.com/owner/repo.git",
        )
        .unwrap();
        let observed = fs::read_to_string(directory.path().join("observed")).unwrap();
        assert!(!observed.contains(token));
        assert!(!observed.contains(&STANDARD.encode(format!("x-access-token:{token}"))));
        assert!(observed.contains("credential.https://github.com.helper=!"));
        assert_eq!(
            fs::read_to_string(directory.path().join("credential")).unwrap(),
            token
        );
    }
    #[test]
    fn failed_results_separate_summary_from_git_diagnostics() {
        let target = PushTarget {
            repository: "owner/repo".into(),
            branch: "main".into(),
            commit: "c".repeat(40),
        };
        let value = super::finished_result(
            "dev",
            "/workspace/repo",
            &target,
            Err("Git push failed (exit status: 1).\nremote: rejected\nmore".into()),
        );
        assert_eq!(value["message"], "Git push failed (exit status: 1).");
        assert_eq!(value["diagnosticDetails"], "remote: rejected\nmore");
        // Results name what was pushed, so a retry pushes the same confirmed target.
        assert_eq!(value["target"]["branch"], "main");
        let plain = super::finished_result(
            "dev",
            "/workspace/repo",
            &target,
            Err("Start the computer.".into()),
        );
        assert_eq!(plain["message"], "Start the computer.");
        assert!(plain.get("diagnosticDetails").is_none());
    }
    #[test]
    fn linux_prefers_the_system_certificate_store() {
        let directory = tempfile::tempdir().unwrap();
        let system = directory.path().join("system.crt");
        let support = directory.path().join("support");
        assert_eq!(
            super::ca_bundle_from(&[system.as_path()], &support),
            support.join("ssl/cacert.pem")
        );
        std::fs::write(&system, b"roots").unwrap();
        assert_eq!(super::ca_bundle_from(&[system.as_path()], &support), system);
    }
    #[test]
    fn requires_computer_repository_paths() {
        assert!(valid_path("/workspace/repo"));
        for path in [
            "/etc",
            "/workspace/../etc",
            "/workspace/repo\nother",
            "/workspace/",
            "/workspace//x",
            "/workspace/./x",
            "/workspace/x/",
        ] {
            assert!(!valid_path(path));
        }
    }
}

#[cfg(test)]
#[path = "host_push_protocol_tests.rs"]
mod protocol_tests;
