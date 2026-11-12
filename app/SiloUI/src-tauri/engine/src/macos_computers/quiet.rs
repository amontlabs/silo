//! Quiet forks on macOS: what a fork turns off before anything in its user session runs.
//!
//! The fork's Data volume is already mounted for the offline rekey, so the scheduled work of
//! the source is moved aside there and nothing of it starts at the fork's boot. Moved files
//! are parked under `Library/Application Support/Silo/Quieted/` with their original paths, not
//! deleted. Scheduled work is parked (jobs that launchd starts on a timer, a watched path or a
//! mount, crontabs, at jobs and periodic scripts), and so is any LaunchAgent or LaunchDaemon
//! that the recipe's `stop` list names by label or program, whatever its schedule. Other
//! services that keep running or start at login stay, since they are part of the environment
//! the fork is meant to have. What needs the running system (stopping named processes and launchd labels) runs first in the
//! fork's first boot (`fork_first_boot.rs`).
use super::{guest_access::shell_quote, offline_setup};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

/// Where parked files go on the Data volume, under their original path.
const PARK_DIR: &str = "Library/Application Support/Silo/Quieted";

/// The launchd jobs Silo installs itself, by exact label. A job stays only when its file is
/// named after one of these labels too; any other label, whatever its prefix, is the user's.
const SILO_JOB_LABELS: [&str; 4] = [
    "com.silo.network-bootstrap",
    "com.silo.proxy-environment",
    "org.silo.computer-use-reconcile",
    "org.silo.guest-recorder",
];

/// The launchd keys that make a job start on its own schedule rather than keep running.
const SCHEDULE_KEYS: [&str; 5] = [
    "StartInterval",
    "StartCalendarInterval",
    "WatchPaths",
    "QueueDirectories",
    "StartOnMount",
];

/// The scripts macOS ships in `/etc/periodic/{daily,weekly,monthly}`; any other script there
/// was added by a user.
const SYSTEM_PERIODIC: &[&str] = &[
    "110.clean-tmps",
    "199.rotate-fax",
    "430.status-rwho",
    "500.daily",
    "320.whatis",
    "400.status-pkg",
    "500.weekly",
    "200.accounting",
    "500.monthly",
    "999.local",
];

const MAX_PLIST_BYTES: u64 = 1024 * 1024;

/// What a fork was made quiet with, kept in its record.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct QuietRecord {
    /// Always true for a fork: scheduled LaunchAgents and LaunchDaemons that are not Silo's or
    /// macOS's, user crontabs, at jobs and the user's periodic scripts are parked.
    pub scheduled_jobs_disabled: bool,
    /// Always true for a fork: a new machine identifier and hostname.
    pub new_identity: bool,
    pub stop: Vec<String>,
    pub scrub: Vec<String>,
    /// Silo secrets copied to the fork; `github` means read-only GitHub access.
    pub secrets: Vec<String>,
    /// Guest paths that were moved aside, in `Library/Application Support/Silo/Quieted/`.
    pub parked: Vec<String>,
    /// Scheduled jobs inside app bundles (registered by the app, not by a plist in a launchd
    /// folder). They are left as they are, since changing a bundle breaks its signature.
    #[serde(default)]
    pub still_scheduled: Vec<String>,
    /// The offline step and the first-boot step both finished.
    pub applied: bool,
}

impl QuietRecord {
    pub(super) fn view(&self) -> silo_api::methods::computers::QuietState {
        silo_api::methods::computers::QuietState {
            applied: self.applied,
            new_identity: self.new_identity,
            scheduled_jobs_disabled: self.scheduled_jobs_disabled,
            stop: self.stop.clone(),
            scrub: self.scrub.clone(),
            secrets: self.secrets.clone(),
            parked: self.parked.clone(),
            still_scheduled: self.still_scheduled.clone(),
        }
    }

    pub(super) fn new(options: &crate::runtime::checkpoints::ForkOptions) -> Self {
        Self {
            scheduled_jobs_disabled: true,
            new_identity: true,
            stop: options.stop.clone(),
            scrub: options.scrub.clone(),
            secrets: options.secrets.clone(),
            parked: Vec::new(),
            still_scheduled: Vec::new(),
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
    let off_volume =
        || format!("{path} is not on the computer's Data volume, so it can't be scrubbed.");
    match parts[0].as_str() {
        "etc" | "var" | "tmp" => parts.insert(0, "private".into()),
        "usr" if parts[1] != "local" => return Err(off_volume()),
        "Users" | "Library" | "Applications" | "opt" | "private" | "usr" | "cores" => {}
        _ => return Err(off_volume()),
    }
    Ok(parts)
}

/// What the scrub never removes, relative to the Data volume (`*` and `?` match within one
/// part). A `true` entry also protects everything below it. Whatever a pattern matches that
/// equals, contains or (for a `true` entry) lies below one of these is refused, so a pattern
/// such as `/Library/*` can't take Silo's files with it.
const PROTECTED: &[(&str, bool)] = &[
    // Folders above the account's home and the places the account lives in.
    ("Users", false),
    ("Users/silo", false),
    ("Applications", false),
    ("opt", false),
    ("usr", false),
    ("usr/local", false),
    ("private", false),
    ("private/tmp", false),
    ("private/var", false),
    // Silo's own files, its launchd jobs and the account's essentials for the first boot.
    ("Library/Application Support/Silo", true),
    ("Users/silo/Library/Application Support/Silo", true),
    ("Library/LaunchDaemons/com.silo.*", true),
    ("Library/LaunchDaemons/org.silo.*", true),
    ("Library/LaunchAgents/com.silo.*", true),
    ("Library/LaunchAgents/org.silo.*", true),
    ("Users/silo/Library/LaunchAgents/com.silo.*", true),
    ("Users/silo/Library/LaunchAgents/org.silo.*", true),
    ("Users/silo/.ssh", true),
    (
        "Users/silo/Library/Preferences/com.apple.SetupAssistant.plist",
        true,
    ),
    ("Library/Preferences/com.apple.loginwindow.plist", true),
    ("Library/Preferences/com.apple.SetupAssistant.plist", true),
    ("Library/Preferences/.GlobalPreferences.plist", true),
    // The account database, SSH, sudo and automatic login.
    ("private/var/db/dslocal", true),
    ("private/var/db/.AppleSetupDone", true),
    ("private/var/db/com.apple.xpc.launchd", true),
    ("private/etc/ssh", true),
    ("private/etc/sudoers", true),
    ("private/etc/sudoers.d", true),
    ("private/etc/kcpassword", true),
    ("private/etc/passwd", true),
    ("private/etc/master.passwd", true),
    ("private/etc/pam.d", true),
];

/// Whether a path (parts below the Data volume, as found) equals, contains or lies below a
/// protected one. APFS volumes are usually case-insensitive, so case is ignored.
fn protected(parts: &[String]) -> bool {
    let lowered: Vec<String> = parts.iter().map(|part| part.to_lowercase()).collect();
    PROTECTED.iter().any(|(pattern, tree)| {
        let pattern: Vec<String> = pattern.split('/').map(str::to_lowercase).collect();
        let shared = lowered.len().min(pattern.len());
        (0..shared).all(|index| matches_glob(&pattern[index], &lowered[index]))
            && (lowered.len() <= pattern.len() || *tree)
    })
}

/// What the offline step did.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Outcome {
    pub parked: Vec<String>,
    pub still_scheduled: Vec<String>,
}

/// Quiets a fork's mounted Data volume: parks the source's scheduled work and deletes the
/// scrub paths. Every scrub path is checked before anything is changed. A repeat finds
/// nothing left to park.
pub(super) fn apply_offline(
    root: &Path,
    scrub: &[String],
    stop: &[String],
) -> Result<Outcome, String> {
    let mut doomed = Vec::new();
    for pattern in scrub {
        let parts = data_volume_components(pattern)?;
        collect_matches(root.to_path_buf(), &parts, &mut Vec::new(), &mut doomed)?;
    }
    let mut outcome = Outcome::default();
    launchd_jobs(root, stop, &mut outcome)?;
    for dir in ["private/var/at/tabs", "private/var/at/jobs"] {
        park_entries(root, dir, &mut outcome, &|_| true)?;
    }
    for frequency in ["daily", "weekly", "monthly"] {
        park_entries(
            root,
            &format!("usr/local/etc/periodic/{frequency}"),
            &mut outcome,
            &|_| true,
        )?;
        park_entries(
            root,
            &format!("private/etc/periodic/{frequency}"),
            &mut outcome,
            &|name| !SYSTEM_PERIODIC.contains(&name),
        )?;
        park(
            root,
            &format!("private/etc/{frequency}.local"),
            &mut outcome,
        )?;
    }
    for found in doomed {
        let removed = if found.is_dir {
            fs::remove_dir_all(&found.path)
        } else {
            fs::remove_file(&found.path)
        };
        match removed {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io("scrub a path", &error)),
        }
    }
    outcome.parked.sort();
    outcome.parked.dedup();
    outcome.still_scheduled.sort();
    outcome.still_scheduled.dedup();
    Ok(outcome)
}

// MARK: Launchd jobs

/// What a launchd job file says about itself.
struct Job {
    label: String,
    file_name: String,
    scheduled: bool,
    program: Option<String>,
}

fn launchd_jobs(root: &Path, stop: &[String], outcome: &mut Outcome) -> Result<(), String> {
    let mut dirs = vec![
        "Library/LaunchAgents".to_string(),
        "Library/LaunchDaemons".to_string(),
    ];
    let users = user_names(root)?;
    dirs.extend(
        users
            .iter()
            .map(|name| format!("Users/{name}/Library/LaunchAgents")),
    );
    for dir in dirs {
        let Some((resolved, path)) = launchd_dir(root, &dir)? else {
            continue;
        };
        // Only the folders macOS itself reads for system-wide jobs hold Apple's own.
        let system = dir == "Library/LaunchAgents" || dir == "Library/LaunchDaemons";
        let mut names = file_names(&path)?;
        names.sort();
        for name in names {
            if !name.ends_with(".plist") || name.starts_with('.') {
                continue;
            }
            let Some(job) = read_job(&path.join(&name), &name) else {
                continue;
            };
            if (job.scheduled || is_stopped(&job, stop)) && !keeps(&job, system) {
                park(root, &format!("{resolved}/{name}"), outcome)?;
            }
        }
    }
    for dir in bundle_job_dirs(root, &users) {
        let Ok(names) = file_names(&root.join(&dir)) else {
            continue;
        };
        for name in names {
            if !name.ends_with(".plist") || name.starts_with('.') {
                continue;
            }
            let file = root.join(&dir).join(&name);
            if read_job(&file, &name).is_some_and(|job| job.scheduled && !keeps(&job, false)) {
                outcome.still_scheduled.push(format!("/{dir}/{name}"));
            }
        }
    }
    Ok(())
}

fn user_names(root: &Path) -> Result<Vec<String>, String> {
    let Some(users) = real_dir(root, "Users")? else {
        return Ok(Vec::new());
    };
    let mut names: Vec<String> = real_subdirs(&users)
        .into_iter()
        .filter(|name| name != "Shared" && !name.starts_with('.'))
        .collect();
    names.sort();
    Ok(names)
}

fn real_subdirs(dir: &Path) -> Vec<String> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect()
}

/// The folders of launchd jobs that apps carry inside their bundles, relative to the volume.
fn bundle_job_dirs(root: &Path, users: &[String]) -> Vec<String> {
    let mut app_dirs = vec!["Applications".to_string()];
    app_dirs.extend(
        users
            .iter()
            .map(|name| format!("Users/{name}/Applications")),
    );
    let mut dirs = Vec::new();
    for app_dir in app_dirs {
        let Ok(Some(path)) = real_dir(root, &app_dir) else {
            continue;
        };
        let mut apps: Vec<String> = real_subdirs(&path)
            .into_iter()
            .filter(|name| name.ends_with(".app"))
            .collect();
        apps.sort();
        for app in apps {
            for kind in ["LaunchAgents", "LaunchDaemons"] {
                dirs.push(format!("{app_dir}/{app}/Contents/Library/{kind}"));
            }
        }
    }
    dirs
}

/// The job file's label, schedule and program; `None` when it can't be read as a launchd
/// property list (launchd can't load it either).
fn read_job(file: &Path, name: &str) -> Option<Job> {
    let small = fs::symlink_metadata(file)
        .is_ok_and(|meta| meta.is_file() && meta.len() <= MAX_PLIST_BYTES);
    if !small {
        return None;
    }
    let value = plist::Value::from_file(file).ok()?;
    let dictionary = value.as_dictionary()?;
    let label = dictionary
        .get("Label")
        .and_then(plist::Value::as_string)
        .map(str::to_owned)
        .unwrap_or_else(|| name.strip_suffix(".plist").unwrap_or(name).to_string());
    let program = dictionary
        .get("Program")
        .and_then(plist::Value::as_string)
        .or_else(|| {
            dictionary
                .get("ProgramArguments")
                .and_then(plist::Value::as_array)
                .and_then(|arguments| arguments.first())
                .and_then(plist::Value::as_string)
        })
        .map(str::to_owned);
    Some(Job {
        label,
        file_name: name.to_string(),
        scheduled: SCHEDULE_KEYS.iter().any(|key| dictionary.contains_key(key)),
        program,
    })
}

/// Whether the recipe's `stop` list names the job: by its label, or by the base name of its
/// program. Such a job is parked whatever its schedule, so it can't start at the fork's boot.
fn is_stopped(job: &Job, stop: &[String]) -> bool {
    let program = job
        .program
        .as_deref()
        .and_then(|program| program.rsplit('/').next());
    stop.iter()
        .any(|name| *name == job.label || program == Some(name.as_str()))
}

/// Silo's jobs stay. macOS's stay only when the file is in a system-wide folder and runs a
/// program of the system: a label alone says nothing about who wrote the file.
fn keeps(job: &Job, system_folder: bool) -> bool {
    if SILO_JOB_LABELS.contains(&job.label.as_str())
        && job.file_name == format!("{}.plist", job.label)
    {
        return true;
    }
    job.label.starts_with("com.apple.")
        && system_folder
        && job.program.as_deref().is_some_and(|program| {
            program.starts_with("/System/")
                || (program.starts_with("/usr/") && !program.starts_with("/usr/local/"))
        })
}

/// A launchd folder below the volume, as a real folder. A link in its place is followed once
/// when its target stays on the Data volume; one that leaves it fails the fork. Returns the
/// folder's own path (after the link) and where it is mounted, or `None` when it is missing.
fn launchd_dir(root: &Path, relative: &str) -> Result<Option<(String, PathBuf)>, String> {
    let (parent, last) = relative.rsplit_once('/').unwrap_or(("", relative));
    let Some(parent_path) = (if parent.is_empty() {
        Some(root.to_path_buf())
    } else {
        real_dir(root, parent)?
    }) else {
        return Ok(None);
    };
    let path = parent_path.join(last);
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io("inspect the computer's files", &error)),
    };
    if meta.is_dir() && !meta.file_type().is_symlink() {
        return Ok(Some((relative.to_string(), path)));
    }
    if !meta.file_type().is_symlink() {
        return Err(format!(
            "Silo could not read /{relative}: it is not a folder."
        ));
    }
    let target = fs::read_link(&path).map_err(|error| io("read a link", &error))?;
    let Some(resolved) = link_target_on_data_volume(parent, &target) else {
        return Err(format!(
            "/{relative} is a link to somewhere other than the computer's Data volume, so its scheduled jobs can't be quieted."
        ));
    };
    let resolved = resolved.join("/");
    Ok(real_dir(root, &resolved)?.map(|path| (resolved, path)))
}

/// Where a link in the guest folder `parent` (relative to the Data volume) points, as parts
/// below the Data volume; `None` when it leaves that volume.
fn link_target_on_data_volume(parent: &str, target: &Path) -> Option<Vec<String>> {
    let mut stack: Vec<String> = Vec::new();
    if !target.is_absolute() {
        stack.extend(
            parent
                .split('/')
                .filter(|part| !part.is_empty())
                .map(str::to_owned),
        );
    }
    for component in target.components() {
        match component {
            Component::RootDir | Component::Prefix(_) => stack.clear(),
            Component::CurDir => {}
            Component::ParentDir => {
                stack.pop()?;
            }
            Component::Normal(name) => stack.push(name.to_str()?.to_owned()),
        }
    }
    data_volume_components(&format!("/{}", stack.join("/"))).ok()
}

/// Parks every entry of a folder the filter accepts, except hidden ones.
fn park_entries(
    root: &Path,
    dir: &str,
    outcome: &mut Outcome,
    accept: &dyn Fn(&str) -> bool,
) -> Result<(), String> {
    let Some(path) = real_dir(root, dir)? else {
        return Ok(());
    };
    let mut names = file_names(&path)?;
    names.sort();
    for name in names
        .into_iter()
        .filter(|name| !name.starts_with('.') && accept(name))
    {
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
            Ok(_) => {
                return Err(
                    "Silo could not park a scheduled job: a folder on the way is a link.".into(),
                )
            }
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
            Ok(_) => {
                return Err(format!(
                    "Silo could not read /{relative}: a folder on the way is a link."
                ))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io("inspect the computer's files", &error)),
        }
    }
    Ok(Some(path))
}

// MARK: Scrub

/// A path a scrub pattern matched, found before anything is removed.
struct Found {
    path: PathBuf,
    is_dir: bool,
}

/// Walks the pattern one part at a time, never through a link, and collects what the last
/// part names. A link at the end is collected itself, not what it points to. Any match that
/// is protected refuses the whole scrub.
fn collect_matches(
    base: PathBuf,
    parts: &[String],
    walked: &mut Vec<String>,
    found: &mut Vec<Found>,
) -> Result<(), String> {
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
            } else {
                if !found.iter().any(|known| known.path == path) {
                    found.push(Found { path, is_dir });
                }
                Ok(())
            }
        } else if is_dir {
            collect_matches(path, rest, walked, found)
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

/// Shell functions to find and stop processes by their exact executable name. `ps` prints the
/// executable's path, so a name is compared whole against its last part: no regular
/// expression, and no cut at the 16 characters of the kernel's process name.
const PROCESS_FUNCTIONS: &str = r#"pids_of() {
  /bin/ps -axo pid=,comm= | /usr/bin/awk -v n="$1" -v me="$$" -v parent="$PPID" '{ pid = $1; sub(/^[ \t]*[0-9]+[ \t]+/, ""); count = split($0, parts, "/"); if (parts[count] == n && pid != me && pid != parent) print pid }'
}
stop_process() {
  pids=$(pids_of "$1")
  [ -z "$pids" ] && return 0
  /bin/kill $pids 2>/dev/null
  i=0
  while [ $i -lt 20 ]; do
    pids=$(pids_of "$1")
    [ -z "$pids" ] && return 0
    i=$((i + 1))
    /bin/sleep 0.5
  done
  /bin/kill -9 $pids 2>/dev/null
  /bin/sleep 1
  [ -z "$(pids_of "$1")" ]
}
loaded() { /bin/launchctl print "$1" >/dev/null 2>&1; }
"#;

/// The root script the first boot runs before anything else: it disables and stops the
/// launchd labels and processes the caller named, and fails when one of them is still loaded
/// or running afterwards. None when nothing is named. A name that matches nothing is not an
/// error.
pub(super) fn runtime_script(stop: &[String]) -> Option<String> {
    if stop.is_empty() {
        return None;
    }
    let mut script = String::from("set +e\nfails=0\n");
    script.push_str(PROCESS_FUNCTIONS);
    for name in stop {
        let n = shell_quote(name);
        let system = shell_quote(&format!("system/{name}"));
        let gui = shell_quote(&format!("gui/{}/{name}", offline_setup::UID));
        script.push_str(&format!(
            "/bin/launchctl disable {system} 2>/dev/null\n\
             /bin/launchctl disable {gui} 2>/dev/null\n\
             /bin/launchctl bootout {system} 2>/dev/null\n\
             /bin/launchctl bootout {gui} 2>/dev/null\n\
             stop_process {n} || {{ echo \"still running: \"{n}; fails=1; }}\n\
             if loaded {system} || loaded {gui}; then echo \"still loaded: \"{n}; fails=1; fi\n",
        ));
    }
    script.push_str("exit $fails\n");
    Some(script)
}

// MARK: Restore

/// Lists what a promote moved back out of the parking folder, so an undo can park it again.
const RESTORED_MARKER: &str = ".restored-by-promote";

/// What a restore did.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Restored {
    /// Guest paths put back where they were.
    pub restored: Vec<String>,
    /// Guest paths still parked: the computer made something of its own at that path.
    pub left: Vec<String>,
}

/// Puts the parked scheduled work of a promoted fork back where it was, except at paths the
/// fork has used for something else since (those stay parked). `parked` is the list the fork's
/// record keeps. Repeating it finds nothing more to move.
pub(super) fn restore_offline(root: &Path, parked: &[String]) -> Result<Restored, String> {
    let mut done = Restored::default();
    let marker = root.join(PARK_DIR).join(RESTORED_MARKER);
    for guest in parked {
        let relative = guest.trim_start_matches('/');
        let parts = data_volume_components(&format!("/{relative}"))?;
        let relative = parts.join("/");
        let from = root.join(PARK_DIR).join(&relative);
        let to = root.join(&relative);
        if fs::symlink_metadata(&from).is_err() {
            // Moved already, or never parked.
            if fs::symlink_metadata(&to).is_ok() {
                done.restored.push(guest.clone());
            }
            continue;
        }
        if fs::symlink_metadata(&to).is_ok() {
            done.left.push(guest.clone());
            continue;
        }
        let parent = to
            .parent()
            .ok_or("Silo could not put a scheduled job back.")?;
        make_real_dirs(root, parent)?;
        remember_restored(&marker, &relative)?;
        fs::rename(&from, &to).map_err(|error| io("put a scheduled job back", &error))?;
        done.restored.push(guest.clone());
    }
    Ok(done)
}

/// Parks again what [`restore_offline`] put back (the promote was undone), unless the
/// computer has since put something at the parking spot.
pub(super) fn repark_offline(root: &Path) -> Result<(), String> {
    let marker = root.join(PARK_DIR).join(RESTORED_MARKER);
    let Ok(text) = fs::read_to_string(&marker) else {
        return Ok(());
    };
    let mut outcome = Outcome::default();
    for relative in text.lines().filter(|line| !line.is_empty()) {
        let parked = root.join(PARK_DIR).join(relative);
        if fs::symlink_metadata(&parked).is_ok() {
            continue;
        }
        park(root, relative, &mut outcome)?;
    }
    fs::remove_file(&marker).map_err(|error| io("finish parking again", &error))
}

fn remember_restored(marker: &Path, relative: &str) -> Result<(), String> {
    use std::io::Write;
    let existing = fs::read_to_string(marker).unwrap_or_default();
    if existing.lines().any(|line| line == relative) {
        return Ok(());
    }
    let parent = marker
        .parent()
        .ok_or("Silo could not put a scheduled job back.")?;
    fs::create_dir_all(parent).map_err(|error| io("make the parking folder", &error))?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(marker)
        .map_err(|error| io("note a scheduled job put back", &error))?;
    writeln!(file, "{relative}").map_err(|error| io("note a scheduled job put back", &error))?;
    file.sync_all()
        .map_err(|error| io("note a scheduled job put back", &error))
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

    fn plist_with_keys(label: &str, keys: &str, program: &str) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict><key>Label</key><string>{label}</string><key>ProgramArguments</key><array><string>{program}</string></array>{keys}</dict></plist>"
        )
    }

    const INTERVAL: &str = "<key>StartInterval</key><integer>60</integer>";

    fn scheduled(label: &str) -> String {
        plist_with_keys(label, INTERVAL, "/opt/homebrew/bin/poll")
    }

    #[test]
    fn only_scheduled_user_jobs_are_parked_and_silo_jobs_stay() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "Users/silo/Library/LaunchAgents/poller.plist",
            &scheduled("dev.poller"),
        );
        write(
            root,
            "Users/silo/Library/LaunchAgents/org.silo.guest-recorder.plist",
            &scheduled("org.silo.guest-recorder"),
        );
        write(
            root,
            "Users/silo/Library/LaunchAgents/org.silo.check.interval.plist",
            &scheduled("org.silo.check.interval"),
        );
        write(
            root,
            "Users/silo/Library/LaunchAgents/db.plist",
            &plist_with_keys(
                "homebrew.mxcl.postgresql",
                "<key>KeepAlive</key><true/><key>RunAtLoad</key><true/>",
                "/opt/homebrew/bin/postgres",
            ),
        );
        write(
            root,
            "Library/LaunchAgents/renamed.plist",
            &scheduled("com.example.helper"),
        );
        write(
            root,
            "Library/LaunchDaemons/com.silo.network-bootstrap.plist",
            &scheduled("com.silo.network-bootstrap"),
        );
        write(root, "Library/LaunchDaemons/not-a-plist.txt", "x");
        write(
            root,
            "Users/other/Library/LaunchAgents/other.plist",
            "not a plist",
        );
        write(
            root,
            "Users/other/Library/LaunchAgents/watch.plist",
            &plist_with_keys(
                "dev.watch",
                "<key>WatchPaths</key><array><string>/tmp/x</string></array>",
                "/bin/true",
            ),
        );
        write(
            root,
            "Users/other/Library/LaunchAgents/cal.plist",
            &plist_with_keys(
                "dev.cal",
                "<key>StartCalendarInterval</key><dict><key>Hour</key><integer>3</integer></dict>",
                "/bin/true",
            ),
        );

        let outcome = apply_offline(root, &[], &[]).unwrap();

        assert_eq!(
            outcome.parked,
            [
                "/Library/LaunchAgents/renamed.plist",
                "/Users/other/Library/LaunchAgents/cal.plist",
                "/Users/other/Library/LaunchAgents/watch.plist",
                "/Users/silo/Library/LaunchAgents/org.silo.check.interval.plist",
                "/Users/silo/Library/LaunchAgents/poller.plist",
            ]
        );
        assert!(!exists(
            root,
            "Users/silo/Library/LaunchAgents/poller.plist"
        ));
        assert!(exists(
            root,
            &format!("{PARK_DIR}/Users/silo/Library/LaunchAgents/poller.plist")
        ));
        assert!(exists(
            root,
            "Users/silo/Library/LaunchAgents/org.silo.guest-recorder.plist"
        ));
        assert!(exists(root, "Users/silo/Library/LaunchAgents/db.plist"));
        assert!(exists(
            root,
            "Library/LaunchDaemons/com.silo.network-bootstrap.plist"
        ));
        assert!(exists(root, "Library/LaunchDaemons/not-a-plist.txt"));
        assert!(exists(root, "Users/other/Library/LaunchAgents/other.plist"));
    }

    #[test]
    fn apple_labels_are_kept_only_for_system_programs_in_system_folders() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "Library/LaunchDaemons/apple-real.plist",
            &plist_with_keys("com.apple.real", INTERVAL, "/usr/libexec/real"),
        );
        write(
            root,
            "Library/LaunchDaemons/apple-system.plist",
            &plist_with_keys("com.apple.sys", INTERVAL, "/System/Library/x"),
        );
        write(
            root,
            "Library/LaunchDaemons/apple-local.plist",
            &plist_with_keys("com.apple.fake", INTERVAL, "/usr/local/bin/mine"),
        );
        write(
            root,
            "Library/LaunchDaemons/apple-home.plist",
            &plist_with_keys("com.apple.fake2", INTERVAL, "/Users/silo/bin/mine"),
        );
        write(root, "Library/LaunchDaemons/apple-none.plist", &format!("<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>Label</key><string>com.apple.none</string>{INTERVAL}</dict></plist>"));
        write(
            root,
            "Users/silo/Library/LaunchAgents/apple-user.plist",
            &plist_with_keys("com.apple.user", INTERVAL, "/usr/bin/true"),
        );

        let outcome = apply_offline(root, &[], &[]).unwrap();

        assert_eq!(
            outcome.parked,
            [
                "/Library/LaunchDaemons/apple-home.plist",
                "/Library/LaunchDaemons/apple-local.plist",
                "/Library/LaunchDaemons/apple-none.plist",
                "/Users/silo/Library/LaunchAgents/apple-user.plist",
            ]
        );
        assert!(exists(root, "Library/LaunchDaemons/apple-real.plist"));
        assert!(exists(root, "Library/LaunchDaemons/apple-system.plist"));
    }

    #[test]
    fn scheduled_jobs_inside_app_bundles_are_listed_and_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let agent = "Applications/Tool.app/Contents/Library/LaunchAgents/tool.plist";
        write(root, agent, &scheduled("dev.tool.agent"));
        write(
            root,
            "Applications/Tool.app/Contents/Library/LaunchDaemons/keep.plist",
            &plist_with_keys("dev.tool.keep", "<key>KeepAlive</key><true/>", "/bin/true"),
        );

        let outcome = apply_offline(root, &[], &[]).unwrap();

        assert!(outcome.parked.is_empty());
        assert_eq!(outcome.still_scheduled, [format!("/{agent}")]);
        assert!(exists(root, agent));
    }

    #[test]
    fn a_launchd_folder_that_is_a_link_is_followed_only_on_the_data_volume() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "Users/silo/dotfiles/agents/poller.plist",
            &scheduled("dev.poller"),
        );
        fs::create_dir_all(root.join("Users/silo/Library")).unwrap();
        std::os::unix::fs::symlink(
            "../dotfiles/agents",
            root.join("Users/silo/Library/LaunchAgents"),
        )
        .unwrap();
        let outcome = apply_offline(root, &[], &[]).unwrap();
        assert_eq!(outcome.parked, ["/Users/silo/dotfiles/agents/poller.plist"]);

        let absolute = tempfile::tempdir().unwrap();
        let root = absolute.path();
        write(
            root,
            "Users/silo/agents/poller.plist",
            &scheduled("dev.poller"),
        );
        fs::create_dir_all(root.join("Users/silo/Library")).unwrap();
        std::os::unix::fs::symlink(
            "/System/Volumes/Data/Users/silo/agents",
            root.join("Users/silo/Library/LaunchAgents"),
        )
        .unwrap();
        assert_eq!(
            apply_offline(root, &[], &[]).unwrap().parked,
            ["/Users/silo/agents/poller.plist"]
        );

        for target in [
            "/System/Library/LaunchAgents",
            "/Volumes/Other/agents",
            "../../../../../x",
        ] {
            let off = tempfile::tempdir().unwrap();
            let root = off.path();
            fs::create_dir_all(root.join("Users/silo/Library")).unwrap();
            std::os::unix::fs::symlink(target, root.join("Users/silo/Library/LaunchAgents"))
                .unwrap();
            let error = apply_offline(root, &[], &[]).unwrap_err();
            assert!(error.contains("Data volume"), "{target}: {error}");
        }
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
        write(root, "private/etc/periodic/daily/800.mine", "#!/bin/sh");
        write(
            root,
            "private/etc/periodic/daily/110.clean-tmps",
            "#!/bin/sh",
        );
        write(
            root,
            "private/etc/periodic/monthly/200.accounting",
            "#!/bin/sh",
        );

        let outcome = apply_offline(root, &[], &[]).unwrap();

        assert_eq!(outcome.parked.len(), 5);
        assert!(!exists(root, "private/var/at/tabs/silo"));
        assert!(exists(
            root,
            &format!("{PARK_DIR}/private/var/at/tabs/silo")
        ));
        assert!(exists(root, "private/var/at/jobs/.SEQ"));
        assert!(!exists(root, "usr/local/etc/periodic/daily/500.mine"));
        assert!(!exists(root, "private/etc/weekly.local"));
        assert!(!exists(root, "private/etc/periodic/daily/800.mine"));
        assert!(exists(root, "private/etc/periodic/daily/110.clean-tmps"));
        assert!(exists(root, "private/etc/periodic/monthly/200.accounting"));
    }

    #[test]
    fn a_disk_with_nothing_scheduled_is_untouched_and_a_repeat_parks_nothing_new() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert_eq!(apply_offline(root, &[], &[]).unwrap(), Outcome::default());
        write(root, "private/var/at/tabs/silo", "x");
        assert_eq!(apply_offline(root, &[], &[]).unwrap().parked.len(), 1);
        assert!(apply_offline(root, &[], &[]).unwrap().parked.is_empty());
    }

    #[test]
    fn a_linked_scheduling_folder_above_the_jobs_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("elsewhere/LaunchAgents")).unwrap();
        std::os::unix::fs::symlink(root.join("elsewhere"), root.join("Library")).unwrap();
        assert!(apply_offline(root, &[], &[]).is_err());
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
        apply_offline(root, &scrub, &[]).unwrap();

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
        let result = apply_offline(root, &["/Users/silo/link/secret".into()], &[]);
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
        write(
            root,
            "Library/Application Support/Silo/NetworkBootstrap/config.json",
            "{}",
        );
        write(root, "Library/Application Support/Other/x", "x");
        write(
            root,
            "Library/LaunchDaemons/com.silo.network-bootstrap.plist",
            "p",
        );
        write(root, "Library/LaunchDaemons/dev.mine.plist", "p");
        write(
            root,
            "private/var/db/dslocal/nodes/Default/users/silo.plist",
            "p",
        );
        write(root, "private/etc/ssh/sshd_config", "c");
        write(root, "private/etc/sudoers", "s");
        write(root, "private/etc/sudoers.d/silo", "s");
        write(root, "Users/silo/.ssh/authorized_keys", "k");
        write(
            root,
            "Users/silo/Library/LaunchAgents/org.silo.guest-recorder.plist",
            "p",
        );
        write(root, "Users/silo/Library/Caches/c", "c");
        for path in [
            "/Users/silo",
            "/Library/Application Support/Silo",
            "/Library/Application Support/Silo/NetworkBootstrap",
            "/Library/Application Support",
            "/Library/*",
            "/Library/Launch*/*",
            "/Library/LaunchDaemons/com.silo.*",
            "/private/var/db/dslocal",
            "/private/var/db/*",
            "/etc/ssh",
            "/etc/ssh/sshd_config",
            "/etc/sudoers.d/*",
            "/etc/*",
            "/Users/silo/.ssh",
            "/Users/silo/Library",
            "/Users/*/Library/LaunchAgents",
            "/Users/silo/Library/LaunchAgents/org.silo.*",
        ] {
            assert!(apply_offline(root, &[path.into()], &[]).is_err(), "{path}");
        }
        for kept in [
            "Users/silo/f",
            "Library/Application Support/Silo/NetworkBootstrap/config.json",
            "Library/Application Support/Other/x",
            "Library/LaunchDaemons/com.silo.network-bootstrap.plist",
            "Library/LaunchDaemons/dev.mine.plist",
            "private/etc/ssh/sshd_config",
            "private/etc/sudoers",
            "private/etc/sudoers.d/silo",
            "Users/silo/.ssh/authorized_keys",
            "Users/silo/Library/LaunchAgents/org.silo.guest-recorder.plist",
        ] {
            assert!(exists(root, kept), "{kept}");
        }
        // Something unrelated under the same folders is fine.
        apply_offline(root, &["/Users/silo/Library/Caches/c".into()], &[]).unwrap();
        apply_offline(root, &["/Library/LaunchDaemons/dev.*".into()], &[]).unwrap();
        assert!(!exists(root, "Users/silo/Library/Caches/c"));
        assert!(!exists(root, "Library/LaunchDaemons/dev.mine.plist"));
    }

    #[test]
    fn protection_ignores_case() {
        let parts = |path: &str| -> Vec<String> { path.split('/').map(str::to_owned).collect() };
        assert!(protected(&parts("library/application support/SILO/x")));
        assert!(protected(&parts("Library/Application Support")));
        assert!(protected(&parts("PRIVATE/ETC/SSH/sshd_config")));
        assert!(!protected(&parts("Library/Caches/x")));
    }

    #[test]
    fn a_scrub_that_cannot_finish_removes_nothing_and_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "Users/silo/logs/a.log", "a");
        write(root, "Users/silo/logs/b.log", "b");
        write(
            root,
            "Library/LaunchDaemons/dev.mine.plist",
            &scheduled("dev.mine"),
        );
        write(root, "Library/Application Support/Silo/x", "x");
        // The first pattern matches; the second is protected.
        let scrub = [
            "/Users/silo/logs/*.log".to_string(),
            "/Library/Application Support/Silo/*".to_string(),
        ];
        assert!(apply_offline(root, &scrub, &[]).is_err());
        assert!(exists(root, "Users/silo/logs/a.log"));
        assert!(exists(root, "Users/silo/logs/b.log"));
        assert!(exists(root, "Library/LaunchDaemons/dev.mine.plist"));
        assert!(!exists(root, PARK_DIR));

        // The second goes through a link.
        write(root, "Users/silo/x/keep", "k");
        std::os::unix::fs::symlink(root.join("Users"), root.join("Users/silo/x/link")).unwrap();
        let scrub = [
            "/Users/silo/logs/a.log".to_string(),
            "/Users/silo/x/link/silo".to_string(),
        ];
        assert!(apply_offline(root, &scrub, &[]).is_err());
        assert!(exists(root, "Users/silo/logs/a.log"));
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
    fn the_runtime_script_stops_each_name_checks_it_and_does_nothing_for_none() {
        assert_eq!(runtime_script(&[]), None);
        let script = runtime_script(&["poller".into(), "dev.x@y".into()]).unwrap();
        assert!(script.contains("launchctl disable 'gui/501/poller'"));
        assert!(script.contains("launchctl bootout 'system/dev.x@y'"));
        assert!(script.contains("stop_process 'poller'"));
        assert!(!script.contains("pkill"));
        assert!(script.contains("still running"));
        assert!(script.contains("if loaded 'system/poller' || loaded 'gui/501/poller'"));
        assert!(script.trim_end().ends_with("exit $fails"));
    }

    /// Starts a copy of `cat` under the given name and returns once it has echoed a byte,
    /// so the process is known to be running under that name. A copied system binary
    /// keeps an invalid signature and is killed at launch until it is signed again.
    fn spawn_running_copy_of_cat(path: &std::path::Path) -> std::process::Child {
        use std::io::{Read, Write};
        fs::copy("/bin/cat", path).unwrap();
        let signed = std::process::Command::new("/usr/bin/codesign")
            .args(["--force", "--sign", "-"])
            .arg(path)
            .output()
            .unwrap();
        assert!(signed.status.success(), "{signed:?}");
        let mut child = std::process::Command::new(path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.as_mut().unwrap().write_all(b"x").unwrap();
        let mut echoed = [0u8; 1];
        child.stdout.as_mut().unwrap().read_exact(&mut echoed).unwrap();
        child
    }

    fn matching_pids(name: &str) -> Vec<u32> {
        let script = format!("{PROCESS_FUNCTIONS}\npids_of {}\n", shell_quote(name));
        let output = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .filter_map(|pid| pid.parse().ok())
            .collect()
    }

    // The stop script runs in a macOS guest; its `ps` output differs on Linux.
    #[cfg(target_os = "macos")]
    #[test]
    fn processes_match_by_exact_name_not_by_prefix_or_pattern() {
        let dir = tempfile::tempdir().unwrap();
        let long = dir.path().join("silo-quiet-test-long-process-name");
        let mut child = spawn_running_copy_of_cat(&long);
        let pid = child.id();
        // The kernel's process name is cut at 16 characters; a name must match whole.
        let found = matching_pids("silo-quiet-test-long-process-name");
        let prefix = matching_pids("silo-quiet-test-");
        let pattern = matching_pids("silo-quiet-test-long-process-nam.");
        let wildcard = matching_pids("silo-quiet-test-.*");
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(found, [pid]);
        assert!(prefix.is_empty() && pattern.is_empty() && wildcard.is_empty());
    }

    #[test]
    fn a_stop_step_ends_the_named_process() {
        let dir = tempfile::tempdir().unwrap();
        let name = "silo-quiet-test-stop-me";
        let path = dir.path().join(name);
        let mut child = spawn_running_copy_of_cat(&path);
        let script = format!(
            "{PROCESS_FUNCTIONS}\nstop_process {} && echo stopped\n",
            shell_quote(name)
        );
        let output = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .output()
            .unwrap();
        let _ = child.wait();
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "stopped");
        assert!(matching_pids(name).is_empty());
    }

    #[test]
    fn the_record_notes_what_was_asked() {
        let options = crate::runtime::checkpoints::ForkOptions {
            secrets: vec!["github".into()],
            stop: vec!["poller".into()],
            scrub: vec!["/Users/silo/.env".into()],
            closed_network: false,
            network_policy: None,
        };
        let record = QuietRecord::new(&options);
        assert!(record.scheduled_jobs_disabled && record.new_identity && !record.applied);
        assert_eq!(record.stop, ["poller"]);
        let text = serde_json::to_string(&record).unwrap();
        assert!(text.contains("scheduledJobsDisabled"));
        // A record saved before the field existed still reads.
        let old = text.replace(",\"stillScheduled\":[]", "");
        assert!(!old.contains("stillScheduled"));
        assert!(serde_json::from_str::<QuietRecord>(&old)
            .unwrap()
            .still_scheduled
            .is_empty());
    }

    #[test]
    fn jobs_named_in_stop_are_parked_whatever_their_schedule() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let keep_alive = "<key>KeepAlive</key><true/><key>RunAtLoad</key><true/>";
        write(
            root,
            "Users/silo/Library/LaunchAgents/by-label.plist",
            &plist_with_keys("dev.poller", keep_alive, "/opt/bin/other"),
        );
        write(
            root,
            "Library/LaunchDaemons/by-program.plist",
            &plist_with_keys("dev.renamed", keep_alive, "/opt/bin/silo-check-poller.sh"),
        );
        write(
            root,
            "Users/silo/Library/LaunchAgents/unrelated.plist",
            &plist_with_keys("dev.db", keep_alive, "/opt/bin/postgres"),
        );
        write(
            root,
            "Users/silo/Library/LaunchAgents/partial.plist",
            &plist_with_keys(
                "dev.partial",
                keep_alive,
                "/opt/bin/silo-check-poller.sh.old",
            ),
        );
        let stop = ["dev.poller".to_string(), "silo-check-poller.sh".to_string()];
        let outcome = apply_offline(root, &[], &stop).unwrap();
        assert!(!exists(
            root,
            "Users/silo/Library/LaunchAgents/by-label.plist"
        ));
        assert!(!exists(root, "Library/LaunchDaemons/by-program.plist"));
        assert!(exists(
            root,
            "Users/silo/Library/LaunchAgents/unrelated.plist"
        ));
        assert!(exists(
            root,
            "Users/silo/Library/LaunchAgents/partial.plist"
        ));
        assert_eq!(outcome.parked.len(), 2);
        assert!(exists(
            root,
            "Library/Application Support/Silo/Quieted/Library/LaunchDaemons/by-program.plist"
        ));
    }

    #[test]
    fn stop_never_parks_silo_jobs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "Library/LaunchDaemons/com.silo.network-bootstrap.plist",
            &plist_with("com.silo.network-bootstrap"),
        );
        let stop = ["com.silo.network-bootstrap".to_string()];
        assert!(apply_offline(root, &[], &stop).unwrap().parked.is_empty());
        assert!(exists(
            root,
            "Library/LaunchDaemons/com.silo.network-bootstrap.plist"
        ));
    }
    fn parked_fork() -> (tempfile::TempDir, Vec<String>) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "Users/silo/Library/LaunchAgents/poller.plist",
            &scheduled("dev.poller"),
        );
        write(root, "private/var/at/tabs/silo", "* * * * * poll\n");
        let parked = apply_offline(root, &[], &[]).unwrap().parked;
        assert_eq!(parked.len(), 2);
        (dir, parked)
    }

    #[test]
    fn a_promote_puts_the_parked_work_back_and_an_undo_parks_it_again() {
        let (dir, parked) = parked_fork();
        let root = dir.path();
        let agent = "Users/silo/Library/LaunchAgents/poller.plist";
        assert!(!exists(root, agent));

        let done = restore_offline(root, &parked).unwrap();
        assert_eq!(done.restored.len(), 2);
        assert!(done.left.is_empty());
        assert!(exists(root, agent));
        assert!(exists(root, "private/var/at/tabs/silo"));
        assert!(!exists(root, &format!("{PARK_DIR}/{agent}")));
        // A repeat finds nothing to move.
        assert!(restore_offline(root, &parked).unwrap().left.is_empty());

        repark_offline(root).unwrap();
        repark_offline(root).unwrap();
        assert!(!exists(root, agent));
        assert!(exists(root, &format!("{PARK_DIR}/{agent}")));
        assert!(exists(
            root,
            &format!("{PARK_DIR}/private/var/at/tabs/silo")
        ));
        assert!(!exists(root, &format!("{PARK_DIR}/{RESTORED_MARKER}")));
    }

    #[test]
    fn work_the_fork_replaced_stays_parked_and_is_not_overwritten() {
        let (dir, parked) = parked_fork();
        let root = dir.path();
        write(
            root,
            "Users/silo/Library/LaunchAgents/poller.plist",
            &scheduled("dev.replacement"),
        );

        let done = restore_offline(root, &parked).unwrap();

        assert_eq!(done.left, ["/Users/silo/Library/LaunchAgents/poller.plist"]);
        assert_eq!(done.restored, ["/private/var/at/tabs/silo"]);
        let text =
            fs::read_to_string(root.join("Users/silo/Library/LaunchAgents/poller.plist")).unwrap();
        assert!(text.contains("dev.replacement"));
        // The undo leaves the replacement alone too.
        repark_offline(root).unwrap();
        let text =
            fs::read_to_string(root.join("Users/silo/Library/LaunchAgents/poller.plist")).unwrap();
        assert!(text.contains("dev.replacement"));
        assert!(exists(
            root,
            &format!("{PARK_DIR}/private/var/at/tabs/silo")
        ));
    }

    #[test]
    fn a_restore_refuses_paths_that_leave_the_data_volume() {
        let dir = tempfile::tempdir().unwrap();
        assert!(restore_offline(dir.path(), &["/Users/../etc/passwd".to_owned()]).is_err());
    }
}
