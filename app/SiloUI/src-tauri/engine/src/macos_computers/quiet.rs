//! Quiet forks on macOS: what a fork turns off before anything in its user session runs.
//!
//! The fork's Data volume is already mounted for the offline rekey, so the scheduled work of
//! the source is moved aside there and nothing of it starts at the fork's boot. Moved files
//! are parked under `Library/Application Support/Silo/Quieted/` with their original paths, not
//! deleted. What needs the running system (stopping named processes and launchd labels) runs
//! first in the fork's first boot (`fork_first_boot.rs`).
use super::guest_access::shell_quote;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

/// Where parked files go on the Data volume, under their original path.
const PARK_DIR: &str = "Library/Application Support/Silo/Quieted";

/// Launchd labels with these prefixes belong to macOS or to Silo and are never disabled.
const KEPT_LABEL_PREFIXES: [&str; 3] = ["com.apple.", "com.silo.", "org.silo."];

const MAX_PLIST_BYTES: u64 = 1024 * 1024;

/// What a fork was made quiet with, kept in its record.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct QuietRecord {
    /// Always true for a fork: LaunchAgents and LaunchDaemons that are not Silo's or Apple's,
    /// user crontabs, at jobs and the user's periodic scripts are parked.
    pub scheduled_jobs_disabled: bool,
    /// Always true for a fork: a new machine identifier and hostname.
    pub new_identity: bool,
    pub stop: Vec<String>,
    pub scrub: Vec<String>,
    /// Silo secrets copied to the fork; `github` means read-only GitHub access.
    pub secrets: Vec<String>,
    /// Guest paths that were moved aside, in `Library/Application Support/Silo/Quieted/`.
    pub parked: Vec<String>,
    /// The offline step and the first-boot step both finished.
    pub applied: bool,
}

impl QuietRecord {
    pub(super) fn new(options: &crate::runtime::checkpoints::ForkOptions) -> Self {
        Self {
            scheduled_jobs_disabled: true,
            new_identity: true,
            stop: options.stop.clone(),
            scrub: options.scrub.clone(),
            secrets: options.secrets.clone(),
            parked: Vec::new(),
            applied: false,
        }
    }
}

/// The same limits the Linux fork applies to what the caller names.
pub(super) fn validate(options: &crate::runtime::checkpoints::ForkOptions) -> Result<(), String> {
    if options.secrets.len() > 64 || options.stop.len() > 64 || options.scrub.len() > 64 {
        return Err("A fork can name at most 64 secrets, processes and paths each.".into());
    }
    if options
        .secrets
        .iter()
        .any(|name| name.is_empty() || name.len() > 128 || name.contains('\0'))
    {
        return Err("A secret name is empty or invalid.".into());
    }
    if options.stop.iter().any(|name| {
        name.is_empty()
            || name.len() > 128
            || name.starts_with('-')
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._@:+-".contains(&b))
    }) {
        return Err(
            "A process or unit to stop may only contain letters, digits and . _ @ : + -.".into(),
        );
    }
    for path in &options.scrub {
        data_volume_components(path)?;
    }
    Ok(())
}

/// The parts of a guest path to scrub, relative to the Data volume. The path must be absolute,
/// name at least two levels, not contain `..`, and live on the Data volume.
fn data_volume_components(path: &str) -> Result<Vec<String>, String> {
    let invalid = || {
        "A path to scrub must be absolute, name at least two levels, and not contain '..'."
            .to_string()
    };
    let mut parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    if !path.starts_with('/')
        || path.len() > 1024
        || path.contains(['\0', '\n'])
        || parts.iter().any(|part| *part == ".." || *part == ".")
        || parts.len() < 2
        || parts[0].contains(['*', '?', '['])
    {
        return Err(invalid());
    }
    if parts.iter().any(|part| part.contains('[')) {
        return Err("A path to scrub may use * and ? but not [.".into());
    }
    if parts.len() >= 3 && parts[..3] == ["System", "Volumes", "Data"] {
        parts.drain(..3);
        if parts.len() < 2 {
            return Err(invalid());
        }
    }
    let mut parts: Vec<String> = parts.into_iter().map(str::to_owned).collect();
    let off_volume = || {
        format!("{path} is not on the computer's Data volume, so it can't be scrubbed.")
    };
    match parts[0].as_str() {
        "etc" | "var" | "tmp" => parts.insert(0, "private".into()),
        "usr" if parts[1] != "local" => return Err(off_volume()),
        "Users" | "Library" | "Applications" | "opt" | "private" | "usr" | "cores" => {}
        _ => return Err(off_volume()),
    }
    Ok(parts)
}

/// Paths the scrub never removes, whatever the pattern: the account database, Silo's own
/// files and every directory above the account's home.
fn protected(parts: &[String]) -> bool {
    let joined = parts.join("/");
    matches!(
        joined.as_str(),
        "Users"
            | "Users/silo"
            | "Library"
            | "Applications"
            | "private"
            | "private/etc"
            | "private/var"
            | "private/var/db"
            | "private/tmp"
            | "usr"
            | "usr/local"
            | "opt"
    ) || joined.starts_with("Library/Application Support/Silo")
        || joined.starts_with("private/var/db/dslocal")
}

/// What the offline step did.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Outcome {
    pub parked: Vec<String>,
}

/// Quiets a fork's mounted Data volume: parks the source's scheduled work and deletes the
/// scrub paths. A repeat finds nothing left to park.
pub(super) fn apply_offline(root: &Path, scrub: &[String]) -> Result<Outcome, String> {
    let mut outcome = Outcome::default();
    launchd_jobs(root, &mut outcome)?;
    for dir in ["private/var/at/tabs", "private/var/at/jobs"] {
        park_entries(root, dir, &mut outcome)?;
    }
    for frequency in ["daily", "weekly", "monthly"] {
        park_entries(
            root,
            &format!("usr/local/etc/periodic/{frequency}"),
            &mut outcome,
        )?;
        park(root, &format!("private/etc/{frequency}.local"), &mut outcome)?;
    }
    for pattern in scrub {
        scrub_pattern(root, pattern)?;
    }
    outcome.parked.sort();
    outcome.parked.dedup();
    Ok(outcome)
}

fn launchd_jobs(root: &Path, outcome: &mut Outcome) -> Result<(), String> {
    let mut dirs = vec![
        "Library/LaunchAgents".to_string(),
        "Library/LaunchDaemons".to_string(),
    ];
    if let Some(users) = real_dir(root, "Users")? {
        let mut names: Vec<String> = fs::read_dir(users)
            .map_err(|error| io("read the users", &error))?
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name != "Shared" && !name.starts_with('.'))
            .collect();
        names.sort();
        dirs.extend(
            names
                .into_iter()
                .map(|name| format!("Users/{name}/Library/LaunchAgents")),
        );
    }
    for dir in dirs {
        let Some(path) = real_dir(root, &dir)? else {
            continue;
        };
        let mut names = file_names(&path)?;
        names.sort();
        for name in names {
            if !name.ends_with(".plist") || name.starts_with('.') {
                continue;
            }
            if keeps_label(&label_of(&path.join(&name), &name)) {
                continue;
            }
            park(root, &format!("{dir}/{name}"), outcome)?;
        }
    }
    Ok(())
}

/// The label a launchd job file declares, or its file name when it can't be read.
fn label_of(file: &Path, name: &str) -> String {
    let stem = name.strip_suffix(".plist").unwrap_or(name).to_string();
    let small = fs::symlink_metadata(file)
        .is_ok_and(|meta| meta.is_file() && meta.len() <= MAX_PLIST_BYTES);
    if !small {
        return stem;
    }
    plist::Value::from_file(file)
        .ok()
        .and_then(|value| {
            value
                .as_dictionary()
                .and_then(|dictionary| dictionary.get("Label"))
                .and_then(plist::Value::as_string)
                .map(str::to_owned)
        })
        .unwrap_or(stem)
}

fn keeps_label(label: &str) -> bool {
    KEPT_LABEL_PREFIXES
        .iter()
        .any(|prefix| label.starts_with(prefix))
}

/// Parks every entry of a folder except hidden ones.
fn park_entries(root: &Path, dir: &str, outcome: &mut Outcome) -> Result<(), String> {
    let Some(path) = real_dir(root, dir)? else {
        return Ok(());
    };
    let mut names = file_names(&path)?;
    names.sort();
    for name in names.into_iter().filter(|name| !name.starts_with('.')) {
        park(root, &format!("{dir}/{name}"), outcome)?;
    }
    Ok(())
}

fn file_names(dir: &Path) -> Result<Vec<String>, String> {
    Ok(fs::read_dir(dir)
        .map_err(|error| io("read a scheduled-jobs folder", &error))?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect())
}

/// Moves `relative` (below the Data volume) to the parking folder, keeping its path. A path
/// that is not there is skipped.
fn park(root: &Path, relative: &str, outcome: &mut Outcome) -> Result<(), String> {
    let from = root.join(relative);
    if fs::symlink_metadata(&from).is_err() {
        return Ok(());
    }
    let to = root.join(PARK_DIR).join(relative);
    let parent = to.parent().ok_or("Silo could not park a scheduled job.")?;
    make_real_dirs(root, parent)?;
    fs::rename(&from, &to).map_err(|error| io("move a scheduled job aside", &error))?;
    outcome.parked.push(format!("/{relative}"));
    Ok(())
}

/// Creates the folders of `dir` below `root`, refusing to pass through a link.
fn make_real_dirs(root: &Path, dir: &Path) -> Result<(), String> {
    let relative = dir
        .strip_prefix(root)
        .map_err(|_| "Silo could not park a scheduled job.".to_string())?;
    let mut path = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err("Silo could not park a scheduled job.".into());
        };
        path.push(name);
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
            Ok(_) => return Err("Silo could not park a scheduled job: a folder on the way is a link.".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&path).map_err(|error| io("make the parking folder", &error))?;
            }
            Err(error) => return Err(io("inspect the parking folder", &error)),
        }
    }
    Ok(())
}

/// `root/relative` when every part is a real folder; `None` when a part is missing.
fn real_dir(root: &Path, relative: &str) -> Result<Option<PathBuf>, String> {
    let mut path = root.to_path_buf();
    for part in relative.split('/') {
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
            Ok(_) => return Err(format!("Silo could not read /{relative}: a folder on the way is a link.")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io("inspect the computer's files", &error)),
        }
    }
    Ok(Some(path))
}

// MARK: Scrub

fn scrub_pattern(root: &Path, pattern: &str) -> Result<(), String> {
    let parts = data_volume_components(pattern)?;
    remove_matches(root.to_path_buf(), &parts, &mut Vec::new())
}

/// Walks the pattern one part at a time, never through a link, and removes what the last
/// part names. A link at the end is removed itself, not what it points to.
fn remove_matches(base: PathBuf, parts: &[String], walked: &mut Vec<String>) -> Result<(), String> {
    let Some((part, rest)) = parts.split_first() else {
        return Ok(());
    };
    let names: Vec<String> = if part.contains(['*', '?']) {
        let mut names = file_names(&base).unwrap_or_default();
        names.retain(|name| matches_glob(part, name));
        names.sort();
        names
    } else {
        vec![part.clone()]
    };
    for name in names {
        let path = base.join(&name);
        let meta = match fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(io("inspect a path to scrub", &error)),
        };
        walked.push(name);
        let is_dir = meta.is_dir() && !meta.file_type().is_symlink();
        let result = if rest.is_empty() {
            if protected(walked) {
                Err(format!("/{} can't be scrubbed.", walked.join("/")))
            } else if is_dir {
                fs::remove_dir_all(&path).map_err(|error| io("scrub a path", &error))
            } else {
                fs::remove_file(&path).map_err(|error| io("scrub a path", &error))
            }
        } else if is_dir {
            remove_matches(path, rest, walked)
        } else if meta.file_type().is_symlink() {
            Err(format!(
                "/{} is a link, so a path through it can't be scrubbed.",
                walked.join("/")
            ))
        } else {
            Ok(())
        };
        walked.pop();
        result?;
    }
    Ok(())
}

/// `*` and `?` within one path part.
fn matches_glob(pattern: &str, name: &str) -> bool {
    fn go(pattern: &[u8], name: &[u8]) -> bool {
        match pattern.split_first() {
            None => name.is_empty(),
            Some((b'*', rest)) => (0..=name.len()).any(|skip| go(rest, &name[skip..])),
            Some((b'?', rest)) => !name.is_empty() && go(rest, &name[1..]),
            Some((byte, rest)) => name.first() == Some(byte) && go(rest, &name[1..]),
        }
    }
    go(pattern.as_bytes(), name.as_bytes())
}

// MARK: Running system

/// The root script the first boot runs before anything else: it disables and stops the
/// launchd labels and processes the caller named. None when nothing is named. A name that
/// matches nothing is not an error.
pub(super) fn runtime_script(stop: &[String]) -> Option<String> {
    if stop.is_empty() {
        return None;
    }
    let mut script = String::from("set +e\n");
    for name in stop {
        let n = shell_quote(name);
        script.push_str(&format!(
            "/bin/launchctl disable system/{name} 2>/dev/null\n\
             /bin/launchctl disable gui/501/{name} 2>/dev/null\n\
             /bin/launchctl bootout system/{name} 2>/dev/null\n\
             /bin/launchctl bootout gui/501/{name} 2>/dev/null\n\
             /usr/bin/pkill -x -- {n} 2>/dev/null\n",
        ));
    }
    script.push_str("exit 0\n");
    Some(script)
}

fn io(action: &str, error: &std::io::Error) -> String {
    super::store::io_error(action, error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, relative: &str, text: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn plist_with(label: &str) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict><key>Label</key><string>{label}</string></dict></plist>"
        )
    }

    fn exists(root: &Path, relative: &str) -> bool {
        fs::symlink_metadata(root.join(relative)).is_ok()
    }

    #[test]
    fn user_launch_jobs_are_parked_and_apple_and_silo_jobs_stay() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "Users/silo/Library/LaunchAgents/poller.plist", &plist_with("dev.poller"));
        write(root, "Users/silo/Library/LaunchAgents/org.silo.guest-recorder.plist", &plist_with("org.silo.guest-recorder"));
        write(root, "Library/LaunchAgents/com.apple.x.plist", &plist_with("com.apple.x"));
        write(root, "Library/LaunchAgents/renamed.plist", &plist_with("com.example.helper"));
        write(root, "Library/LaunchDaemons/com.silo.network-bootstrap.plist", &plist_with("com.silo.network-bootstrap"));
        write(root, "Library/LaunchDaemons/not-a-plist.txt", "x");
        write(root, "Users/other/Library/LaunchAgents/other.plist", "not a plist");

        let outcome = apply_offline(root, &[]).unwrap();

        assert_eq!(
            outcome.parked,
            [
                "/Library/LaunchAgents/renamed.plist",
                "/Users/other/Library/LaunchAgents/other.plist",
                "/Users/silo/Library/LaunchAgents/poller.plist",
            ]
        );
        assert!(!exists(root, "Users/silo/Library/LaunchAgents/poller.plist"));
        assert!(exists(root, &format!("{PARK_DIR}/Users/silo/Library/LaunchAgents/poller.plist")));
        assert!(exists(root, "Users/silo/Library/LaunchAgents/org.silo.guest-recorder.plist"));
        assert!(exists(root, "Library/LaunchAgents/com.apple.x.plist"));
        assert!(exists(root, "Library/LaunchDaemons/com.silo.network-bootstrap.plist"));
        assert!(exists(root, "Library/LaunchDaemons/not-a-plist.txt"));
    }

    #[test]
    fn crontabs_at_jobs_and_user_periodic_scripts_are_parked() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "private/var/at/tabs/silo", "* * * * * poll\n");
        write(root, "private/var/at/jobs/a00001", "job");
        write(root, "private/var/at/jobs/.SEQ", "seq");
        write(root, "usr/local/etc/periodic/daily/500.mine", "#!/bin/sh");
        write(root, "private/etc/weekly.local", "#!/bin/sh");

        let outcome = apply_offline(root, &[]).unwrap();

        assert_eq!(outcome.parked.len(), 4);
        assert!(!exists(root, "private/var/at/tabs/silo"));
        assert!(exists(root, &format!("{PARK_DIR}/private/var/at/tabs/silo")));
        assert!(exists(root, "private/var/at/jobs/.SEQ"));
        assert!(!exists(root, "usr/local/etc/periodic/daily/500.mine"));
        assert!(!exists(root, "private/etc/weekly.local"));
    }

    #[test]
    fn a_disk_with_nothing_scheduled_is_untouched_and_a_repeat_parks_nothing_new() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert_eq!(apply_offline(root, &[]).unwrap(), Outcome::default());
        write(root, "private/var/at/tabs/silo", "x");
        assert_eq!(apply_offline(root, &[]).unwrap().parked.len(), 1);
        assert!(apply_offline(root, &[]).unwrap().parked.is_empty());
    }

    #[test]
    fn a_linked_scheduling_folder_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("elsewhere")).unwrap();
        fs::create_dir_all(root.join("Library")).unwrap();
        std::os::unix::fs::symlink(root.join("elsewhere"), root.join("Library/LaunchAgents"))
            .unwrap();
        assert!(apply_offline(root, &[]).is_err());
    }

    #[test]
    fn scrub_paths_are_deleted_and_links_are_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "Users/silo/proj/.env", "TOKEN=1");
        write(root, "Users/silo/proj/keep.txt", "k");
        write(root, "Users/silo/logs/a.log", "a");
        write(root, "Users/silo/logs/b.log", "b");
        write(root, "Users/silo/logs/c.txt", "c");
        write(root, "outside/secret", "s");
        std::os::unix::fs::symlink(root.join("outside"), root.join("Users/silo/link")).unwrap();
        write(root, "private/etc/tool.conf", "c");

        let scrub = [
            "/Users/silo/proj/.env",
            "/Users/silo/logs/*.log",
            "/Users/silo/link",
            "/Users/silo/missing/file",
            "/etc/tool.conf",
        ]
        .map(String::from);
        apply_offline(root, &scrub).unwrap();

        assert!(!exists(root, "Users/silo/proj/.env"));
        assert!(exists(root, "Users/silo/proj/keep.txt"));
        assert!(!exists(root, "Users/silo/logs/a.log"));
        assert!(!exists(root, "Users/silo/logs/b.log"));
        assert!(exists(root, "Users/silo/logs/c.txt"));
        assert!(!exists(root, "Users/silo/link"));
        assert!(exists(root, "outside/secret"));
        assert!(!exists(root, "private/etc/tool.conf"));
    }

    #[test]
    fn scrub_refuses_a_path_through_a_link() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "outside/secret", "s");
        fs::create_dir_all(root.join("Users/silo")).unwrap();
        std::os::unix::fs::symlink(root.join("outside"), root.join("Users/silo/link")).unwrap();
        let result = apply_offline(root, &["/Users/silo/link/secret".into()]);
        assert!(result.is_err());
        assert!(exists(root, "outside/secret"));
    }

    #[test]
    fn scrub_rules_match_the_linux_validation() {
        let check = |path: &str| {
            validate(&crate::runtime::checkpoints::ForkOptions {
                scrub: vec![path.into()],
                ..Default::default()
            })
        };
        assert!(check("/Users/silo/.env").is_ok());
        assert!(check("/System/Volumes/Data/Users/silo/x").is_ok());
        assert!(check("Users/silo/x").is_err());
        assert!(check("/Users/silo/../x").is_err());
        assert!(check("/Users").is_err());
        assert!(check("/*/silo").is_err());
        assert!(check("/Users/silo/[a]").is_err());
        assert!(check("/System/Library/x").is_err());
        assert!(check("/usr/bin/x").is_err());
        assert!(check("/usr/local/bin/x").is_ok());
    }

    #[test]
    fn scrub_never_removes_the_account_or_silo_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "Users/silo/f", "f");
        write(root, "Library/Application Support/Silo/NetworkBootstrap/config.json", "{}");
        write(root, "private/var/db/dslocal/nodes/Default/users/silo.plist", "p");
        for path in [
            "/Users/silo",
            "/Library/Application Support/Silo",
            "/private/var/db/dslocal",
        ] {
            assert!(apply_offline(root, &[path.into()]).is_err(), "{path}");
        }
        assert!(exists(root, "Users/silo/f"));
        assert!(exists(
            root,
            "Library/Application Support/Silo/NetworkBootstrap/config.json"
        ));
    }

    #[test]
    fn validation_limits_names_like_linux() {
        let options = |stop: &str| crate::runtime::checkpoints::ForkOptions {
            stop: vec![stop.into()],
            ..Default::default()
        };
        assert!(validate(&options("com.example.poller")).is_ok());
        assert!(validate(&options("-x")).is_err());
        assert!(validate(&options("a b")).is_err());
        assert!(validate(&options("")).is_err());
    }

    #[test]
    fn the_runtime_script_stops_each_name_and_does_nothing_for_none() {
        assert_eq!(runtime_script(&[]), None);
        let script = runtime_script(&["poller".into(), "dev.x@y".into()]).unwrap();
        assert!(script.contains("launchctl disable gui/501/poller"));
        assert!(script.contains("launchctl bootout system/dev.x@y"));
        assert!(script.contains("pkill -x -- 'poller'"));
        assert!(script.trim_end().ends_with("exit 0"));
    }

    #[test]
    fn the_record_notes_what_was_asked() {
        let options = crate::runtime::checkpoints::ForkOptions {
            secrets: vec!["github".into()],
            stop: vec!["poller".into()],
            scrub: vec!["/Users/silo/.env".into()],
        };
        let record = QuietRecord::new(&options);
        assert!(record.scheduled_jobs_disabled && record.new_identity && !record.applied);
        assert_eq!(record.stop, ["poller"]);
        let text = serde_json::to_string(&record).unwrap();
        assert!(text.contains("scheduledJobsDisabled"));
    }
}
