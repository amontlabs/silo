//! Read configured author defaults without opening a repository or changing device settings.
use serde::Serialize;
use std::{
    ffi::OsString,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, PoisonError},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DeviceIdentity {
    pub name: String,
    pub email: String,
}

/// How long a read identity is used before it is read again in the background.
const CACHE_TTL: Duration = Duration::from_secs(60);
static CACHE: Cache = Cache::new();

/// The last known device identity, without waiting. Reading runs Git and Jujutsu
/// processes, so it never happens on the caller's thread (which may hold locks):
/// a missing or stale value is refreshed by one background read, and `changed`
/// runs if that read finds a different identity.
pub fn cached(changed: impl FnOnce() + Send + 'static) -> Option<DeviceIdentity> {
    CACHE.get(Instant::now(), CACHE_TTL, read, changed)
}

struct Cache(Mutex<CacheState>);
struct CacheState {
    value: Option<DeviceIdentity>,
    read_at: Option<Instant>,
    reading: bool,
}
impl Cache {
    const fn new() -> Self {
        Self(Mutex::new(CacheState {
            value: None,
            read_at: None,
            reading: false,
        }))
    }
    fn state(&self) -> std::sync::MutexGuard<'_, CacheState> {
        // A cache of a value re-read from the device; recover after a panic (K-24).
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
    fn get(
        &'static self,
        now: Instant,
        ttl: Duration,
        read: impl FnOnce() -> Option<DeviceIdentity> + Send + 'static,
        changed: impl FnOnce() + Send + 'static,
    ) -> Option<DeviceIdentity> {
        let mut state = self.state();
        let stale = state
            .read_at
            .is_none_or(|at| now.saturating_duration_since(at) >= ttl);
        if stale && !state.reading {
            state.reading = true;
            let refresh = thread::Builder::new()
                .name("device-identity".into())
                .spawn(move || self.refresh(read, changed));
            if refresh.is_err() {
                state.reading = false;
            }
        }
        state.value.clone()
    }
    fn refresh(&self, read: impl FnOnce() -> Option<DeviceIdentity>, changed: impl FnOnce()) {
        struct Reading<'a>(&'a Cache);
        impl Drop for Reading<'_> {
            fn drop(&mut self) {
                self.0.state().reading = false;
            }
        }
        let _reading = Reading(self);
        let value = read();
        let differs = {
            let mut state = self.state();
            state.read_at = Some(Instant::now());
            let differs = state.value != value;
            state.value = value;
            differs
        };
        if differs {
            changed();
        }
    }
}

fn read() -> Option<DeviceIdentity> {
    let home = std::env::var_os("HOME")?;
    let mut directories: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|value| {
            std::env::split_paths(&value)
                .filter(|path| path.is_absolute())
                .collect()
        })
        .unwrap_or_default();
    directories.extend([
        PathBuf::from(&home).join(".local/bin"),
        PathBuf::from(&home).join(".cargo/bin"),
    ]);
    directories
        .extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"].map(PathBuf::from));
    let environment: Vec<(OsString, OsString)> = [
        "HOME",
        "XDG_CONFIG_HOME",
        "GIT_CONFIG_GLOBAL",
        "JJ_CONFIG",
        "JJ_USER",
        "JJ_EMAIL",
    ]
    .iter()
    .filter_map(|key| std::env::var_os(key).map(|value| ((*key).into(), value)))
    .collect();
    read_with(|tool, key| {
        let executable = directories.iter().map(|dir| dir.join(tool)).find(|path| {
            use std::os::unix::fs::PermissionsExt;
            path.metadata().is_ok_and(|metadata| {
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            }) && !installer_shim(path)
        })?;
        let args = if tool == "git" {
            vec!["config", "--global", "--includes", "--get", key]
        } else {
            vec!["--ignore-working-copy", "--no-pager", "config", "get", key]
        };
        query(&executable, &args, &environment, Duration::from_secs(2))
    })
}

/// On macOS `/usr/bin/git` is a shim that opens the Command Line Tools installer
/// dialog when no developer directory provides Git. An optional default must never
/// open that dialog, so the shim is skipped unless a developer Git exists.
fn installer_shim(path: &Path) -> bool {
    cfg!(target_os = "macos") && installer_shim_with(path, &developer_directories())
}
fn installer_shim_with(path: &Path, directories: &[PathBuf]) -> bool {
    (path == Path::new("/usr/bin/git")
        || path
            .canonicalize()
            .is_ok_and(|resolved| resolved == Path::new("/usr/bin/git")))
        && !developer_git_installed(directories)
}
fn developer_directories() -> Vec<PathBuf> {
    // `xcode-select --switch` records its choice in this link; the defaults follow.
    std::fs::read_link("/var/db/xcode_select_link")
        .into_iter()
        .chain([
            PathBuf::from("/Applications/Xcode.app/Contents/Developer"),
            PathBuf::from("/Library/Developer/CommandLineTools"),
        ])
        .collect()
}
fn developer_git_installed(directories: &[PathBuf]) -> bool {
    directories
        .iter()
        .any(|directory| directory.join("usr/bin/git").is_file())
}

fn read_with(mut get: impl FnMut(&str, &str) -> Option<String>) -> Option<DeviceIdentity> {
    for tool in ["git", "jj"] {
        let name = get(tool, "user.name").and_then(valid_value);
        let email = get(tool, "user.email").and_then(valid_value);
        if let (Some(name), Some(email)) = (name, email) {
            return Some(DeviceIdentity { name, email });
        }
    }
    None
}

fn valid_value(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.len() <= 1024 && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
}

fn query(
    executable: &Path,
    args: &[&str],
    environment: &[(OsString, OsString)],
    timeout: Duration,
) -> Option<String> {
    // Root cannot inherit the app checkout's Git/Jujutsu repository configuration.
    let mut output = tempfile::tempfile().ok()?;
    let mut command = Command::new(executable);
    command
        .args(args)
        .current_dir("/")
        .env_clear()
        .envs(environment.iter().cloned())
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::from(output.try_clone().ok()?))
        .stderr(Stdio::null());
    use std::os::unix::process::CommandExt;
    command.process_group(0);
    let mut child = command.spawn().ok()?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                break;
            }
            Ok(None)
                if Instant::now() < deadline
                    && output
                        .metadata()
                        .is_ok_and(|metadata| metadata.len() <= 4096) =>
            {
                thread::sleep(Duration::from_millis(10))
            }
            _ => {
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                let _ = child.wait();
                return None;
            }
        }
    }
    output.seek(SeekFrom::Start(0)).ok()?;
    let mut bytes = Vec::new();
    output.take(4097).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 4096 {
        return None;
    }
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(name: &str) -> Option<DeviceIdentity> {
        Some(DeviceIdentity {
            name: name.into(),
            email: "author@example.test".into(),
        })
    }

    #[test]
    fn cached_identity_is_read_once_in_the_background_and_refreshed_after_its_ttl() {
        let cache: &'static Cache = Box::leak(Box::new(Cache::new()));
        let start = Instant::now();
        let ttl = Duration::from_secs(60);
        let (release, released) = std::sync::mpsc::channel::<()>();
        let (notify, notified) = std::sync::mpsc::channel();
        let first = notify.clone();
        // The caller never waits for Git: the first call returns immediately.
        assert_eq!(
            cache.get(
                start,
                ttl,
                move || {
                    released.recv().unwrap();
                    identity("First")
                },
                move || first.send("first").unwrap()
            ),
            None
        );
        // Callers during the read do not start more Git processes.
        assert_eq!(
            cache.get(
                start,
                ttl,
                || panic!("second concurrent read"),
                || panic!("second change")
            ),
            None
        );
        release.send(()).unwrap();
        assert_eq!(
            notified.recv_timeout(Duration::from_secs(5)).unwrap(),
            "first"
        );
        while cache.state().reading {
            thread::sleep(Duration::from_millis(5));
        }
        // A fresh value is served without reading again.
        let read_at = cache.state().read_at.unwrap();
        assert_eq!(
            cache.get(
                read_at,
                ttl,
                || panic!("read within ttl"),
                || panic!("change")
            ),
            identity("First")
        );
        // A stale value is still served while one background refresh runs; an
        // unchanged identity does not announce a change.
        let (done, finished) = std::sync::mpsc::channel();
        assert_eq!(
            cache.get(
                read_at + ttl,
                ttl,
                move || {
                    done.send(()).unwrap();
                    identity("First")
                },
                || panic!("unchanged identity announced")
            ),
            identity("First")
        );
        finished.recv_timeout(Duration::from_secs(5)).unwrap();
        while cache.state().reading {
            thread::sleep(Duration::from_millis(5));
        }
        let later = cache.state().read_at.unwrap() + ttl;
        let changed = notify.clone();
        cache.get(
            later,
            ttl,
            || identity("Second"),
            move || changed.send("second").unwrap(),
        );
        assert_eq!(
            notified.recv_timeout(Duration::from_secs(5)).unwrap(),
            "second"
        );
    }

    #[test]
    fn git_installer_shim_is_used_only_with_developer_tools() {
        let root = tempfile::tempdir().unwrap();
        let tools = root.path().join("CommandLineTools");
        assert!(!developer_git_installed(&[tools.clone()]));
        std::fs::create_dir_all(tools.join("usr/bin")).unwrap();
        std::fs::write(tools.join("usr/bin/git"), b"").unwrap();
        assert!(developer_git_installed(&[
            root.path().join("missing"),
            tools
        ]));
        assert!(!installer_shim(Path::new("/opt/homebrew/bin/git")));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn git_shortcuts_do_not_bypass_the_installer_shim_check() {
        let root = tempfile::tempdir().unwrap();
        let shortcut = root.path().join("git");
        std::os::unix::fs::symlink("/usr/bin/git", &shortcut).unwrap();
        let missing_tools = [root.path().join("missing-developer-tools")];
        assert!(installer_shim_with(&shortcut, &missing_tools));
        let tools = root.path().join("CommandLineTools");
        std::fs::create_dir_all(tools.join("usr/bin")).unwrap();
        std::fs::write(tools.join("usr/bin/git"), b"developer git").unwrap();
        assert!(!installer_shim_with(&shortcut, &[tools]));
    }

    #[test]
    fn prefers_complete_git_and_never_combines_tools() {
        let identity = read_with(|tool, key| {
            assert_eq!(tool, "git");
            Some(
                if key == "user.name" {
                    "Git Author\n"
                } else {
                    "git@example.com\n"
                }
                .into(),
            )
        })
        .unwrap();
        assert_eq!(identity.name, "Git Author");
        let fallback = read_with(|tool, key| match (tool, key) {
            ("git", "user.name") => Some("Git Author".into()),
            ("jj", "user.name") => Some("JJ Author".into()),
            ("jj", "user.email") => Some("jj@example.com".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(fallback.name, "JJ Author");
        assert_eq!(fallback.email, "jj@example.com");
    }

    #[test]
    fn absent_empty_and_malformed_values_never_become_identity() {
        assert!(read_with(|_, _| None).is_none());
        for value in ["", " \n", "name\nsecond", "name\u{1b}[31m"] {
            assert!(read_with(|_, _| Some(value.into())).is_none());
        }
        assert!(read_with(|_, _| Some("x".repeat(1025))).is_none());
    }

    #[test]
    fn subprocess_failure_timeout_and_excess_output_are_unavailable() {
        assert!(query(
            Path::new("/bin/sh"),
            &["-c", "exit 1"],
            &[],
            Duration::from_secs(1)
        )
        .is_none());
        assert!(query(
            Path::new("/bin/sh"),
            &["-c", "sleep 2"],
            &[],
            Duration::from_millis(30)
        )
        .is_none());
        assert!(query(
            Path::new("/bin/sh"),
            &["-c", "while :; do printf 'too much output'; done"],
            &[],
            Duration::from_secs(1)
        )
        .is_none());
    }

    #[test]
    fn git_reads_included_global_identity_without_repository_or_host_settings() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("included"),
            "[user]\n name = Included Author\n email = included@example.com\n",
        )
        .unwrap();
        std::fs::write(
            root.path().join(".gitconfig"),
            "[include]\n path = included\n",
        )
        .unwrap();
        let environment = vec![("HOME".into(), root.path().as_os_str().to_owned())];
        let name = query(
            Path::new("/usr/bin/git"),
            &["config", "--global", "--includes", "--get", "user.name"],
            &environment,
            // This test checks Git include semantics, not launch latency. The
            // macOS /usr/bin/git shim can start slowly on a loaded CI runner.
            // Timeout enforcement has its own deterministic subprocess test.
            Duration::from_secs(15),
        )
        .expect("isolated Git config query should read the included identity");
        assert_eq!(name.trim(), "Included Author");
    }
}
