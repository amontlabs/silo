//! Computer use inside the guest: the official ChatGPT macOS app and LCU's macOS build,
//! with the Accessibility and Screen Recording grants they need.
//!
//! The host downloads the two pinned archives (`guest/macos/*-lock.json`) into its own cache,
//! checks their SHA-256, copies them with `guest/macos/silo-computer-use.zsh` into the running
//! computer and runs the script there as `silo`. The script does the installation, writes the
//! TCC rows and registers LCU's agents; see its header for the steps.
use super::guest_access::{self as access, CommandOutput};
use super::store::Layout;
use super::{
    app_data, layout_and_record, registry, restore_image, set_detail, setup_log, store, templates,
};
use crate::computer_use;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Mutex},
    time::Duration,
};
use tauri::AppHandle;

const APP_LOCK: &str = include_str!("../../guest/macos/chatgpt-app-lock.json");
const LCU_LOCK: &str = include_str!("../../guest/macos/lcu-lock.json");
const SCRIPT: &str = include_str!("../../guest/macos/silo-computer-use.zsh");
const SCRIPT_NAME: &str = "silo-computer-use.zsh";
const PINNED_NAME: &str = "pinned.json";
/// Where the files are staged in the guest. The script keeps its work folder inside it.
const STAGE: &str = "/private/tmp/silo-computer-use";
const CACHE_DIR: &str = "macos-computer-use";
const ARCHITECTURE: &str = "arm64";
const APP_HOST: &str = "persistent.oaistatic.com";
const LCU_HOST: &str = "github.com";
const MAX_ARCHIVE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const SMALL_COPY: Duration = Duration::from_secs(120);
const APP_COPY: Duration = Duration::from_secs(1800);
const QUICK_COMMAND: Duration = Duration::from_secs(60);
const APPLY_TIMEOUT: Duration = Duration::from_secs(1800);
/// Lines of the script's output a failure quotes.
const QUOTE_LINES: usize = 3;

/// How long a started computer may take to accept SSH logins before its update gives up.
const UPDATE_SSH_WAIT: Duration = Duration::from_secs(600);
/// How long a Delete waits for a running update to notice and end.
const UPDATE_DRAIN: Duration = Duration::from_secs(60);
/// The row detail while computer use is being updated.
const UPDATING: &str = "Updating Computer Use";

/// A running update's place in `UPDATES`, and the Start whose update waits behind it.
struct Slot {
    token: u64,
    queued: Option<u64>,
}

/// Computers whose computer use is being updated.
static UPDATES: Mutex<Option<HashMap<String, Slot>>> = Mutex::new(None);
static NEXT_TOKEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn updates() -> std::sync::MutexGuard<'static, Option<HashMap<String, Slot>>> {
    UPDATES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The right to run the updates of one computer, one at a time. It carries a token, so a
/// claim that has ended can never remove the place of a newer one.
struct UpdateClaim {
    id: String,
    token: u64,
}

/// What a request for an update came to.
enum Request {
    /// This request runs now.
    Run(UpdateClaim),
    /// An update is running; this request waits behind it (the highest Start wins).
    Queued,
    /// The request is not from the Start that owns the computer: nothing to do.
    Stale,
}

impl UpdateClaim {
    /// Asks for the update of Start number `attempt`. `owns` says whether that Start still
    /// owns the running computer; a request from any other is refused before it can queue.
    fn request(id: &str, attempt: u64, owns: bool) -> Request {
        if !owns {
            return Request::Stale;
        }
        let mut updates = updates();
        let map = updates.get_or_insert_with(HashMap::new);
        match map.get_mut(id) {
            Some(slot) => {
                slot.queued = Some(slot.queued.map_or(attempt, |queued| queued.max(attempt)));
                Request::Queued
            }
            None => {
                let token = NEXT_TOKEN.fetch_add(1, Ordering::SeqCst);
                map.insert(
                    id.to_string(),
                    Slot {
                        token,
                        queued: None,
                    },
                );
                Request::Run(Self {
                    id: id.to_string(),
                    token,
                })
            }
        }
    }

    /// The queued request to run next; with none, the claim is given up in the same step,
    /// so a request cannot slip in between.
    fn next_or_release(&self) -> Option<u64> {
        let mut updates = updates();
        let map = updates.get_or_insert_with(HashMap::new);
        let queued = map
            .get_mut(&self.id)
            .filter(|slot| slot.token == self.token)
            .and_then(|slot| slot.queued.take());
        if queued.is_none() {
            Self::remove_own(map, &self.id, self.token);
        }
        queued
    }

    fn remove_own(map: &mut HashMap<String, Slot>, id: &str, token: u64) {
        if map.get(id).is_some_and(|slot| slot.token == token) {
            map.remove(id);
        }
    }
}

impl Drop for UpdateClaim {
    fn drop(&mut self) {
        if let Some(map) = updates().as_mut() {
            Self::remove_own(map, &self.id, self.token);
        }
    }
}

/// Waits until no update of computer `id` is running, for a Delete that has already made
/// the update's liveness check fail: the update must not touch the computer's folder
/// after it is removed. Fails when the update has not ended in time; the files stay.
pub(super) fn wait_for_update_end(id: &str) -> Result<(), String> {
    wait_for_update_end_within(id, UPDATE_DRAIN)
}

fn wait_for_update_end_within(id: &str, limit: Duration) -> Result<(), String> {
    let deadline = std::time::Instant::now() + limit;
    while updates().as_ref().is_some_and(|map| map.contains_key(id)) {
        if std::time::Instant::now() >= deadline {
            return Err(
                "Computer Use is still being updated in this computer. Try deleting it again in a moment."
                    .into(),
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

/// Serializes downloads so two computers never write the same partial file.
static DOWNLOAD_TURN: Mutex<()> = Mutex::new(());

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Asset {
    url: String,
    sha256: String,
    bytes: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppLock {
    schema_version: u32,
    version: String,
    bundle_identifier: String,
    team_identifier: String,
    lcu_version: String,
    architectures: std::collections::HashMap<String, Asset>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LcuLock {
    schema_version: u32,
    version: String,
    assets: std::collections::HashMap<String, Asset>,
}

/// What this build installs in a computer.
#[derive(Debug)]
struct Pins {
    app: AppLock,
    lcu: LcuLock,
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn valid_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value.starts_with(|c: char| c.is_ascii_digit())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.')
}

fn checked_asset<'a>(
    assets: &'a std::collections::HashMap<String, Asset>,
    host: &str,
) -> Result<&'a Asset, String> {
    let invalid = || "Silo's computer use information is invalid.".to_string();
    let asset = assets.get(ARCHITECTURE).ok_or_else(invalid)?;
    let url = reqwest::Url::parse(&asset.url).map_err(|_| invalid())?;
    if url.scheme() != "https"
        || url.host_str() != Some(host)
        || !valid_sha256(&asset.sha256)
        || asset.bytes == 0
        || asset.bytes > MAX_ARCHIVE_BYTES
        || file_name(&asset.url).is_none()
    {
        return Err(invalid());
    }
    Ok(asset)
}

/// The last path component of `url` when it is a plain file name.
fn file_name(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let name = parsed.path_segments()?.next_back()?.to_string();
    let plain = !name.is_empty()
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    plain.then_some(name)
}

impl Pins {
    fn parse(app: &str, lcu: &str) -> Result<Self, String> {
        let invalid = || "Silo's computer use information is invalid.".to_string();
        let pins = Self {
            app: serde_json::from_str(app).map_err(|_| invalid())?,
            lcu: serde_json::from_str(lcu).map_err(|_| invalid())?,
        };
        if pins.app.schema_version != 1
            || pins.lcu.schema_version != 1
            || !valid_version(&pins.app.version)
            || !valid_version(&pins.lcu.version)
            || pins.app.lcu_version != pins.lcu.version
            || pins.app.bundle_identifier != "com.openai.codex"
            || pins.app.team_identifier.len() != 10
            || !pins
                .app
                .team_identifier
                .bytes()
                .all(|b| b.is_ascii_alphanumeric())
        {
            return Err(invalid());
        }
        checked_asset(&pins.app.architectures, APP_HOST)?;
        checked_asset(&pins.lcu.assets, LCU_HOST)?;
        Ok(pins)
    }

    fn bundled() -> Result<Self, String> {
        Self::parse(APP_LOCK, LCU_LOCK)
    }

    fn app_asset(&self) -> &Asset {
        &self.app.architectures[ARCHITECTURE]
    }

    fn lcu_asset(&self) -> &Asset {
        &self.lcu.assets[ARCHITECTURE]
    }

    /// The file the script reads its pins from.
    fn pinned_json(&self) -> String {
        serde_json::json!({
            "chatgptVersion": self.app.version,
            "chatgptArchive": file_name(&self.app_asset().url),
            "chatgptSha256": self.app_asset().sha256,
            "bundleIdentifier": self.app.bundle_identifier,
            "teamIdentifier": self.app.team_identifier,
            "lcuVersion": self.lcu.version,
            "lcuArchive": file_name(&self.lcu_asset().url),
            "lcuSha256": self.lcu_asset().sha256,
        })
        .to_string()
    }
}

/// The pinned archives and the script, which decide what a computer holds after setup.
pub(super) fn setup_inputs() -> [&'static str; 3] {
    [APP_LOCK, LCU_LOCK, SCRIPT]
}

/// How a run reports progress and learns that it must end. Setup of a new computer ties both
/// to the creation; an update of a running computer ties them to that Start.
pub(super) struct Job<'a> {
    /// False once the run must stop: the computer was stopped, deleted or restored, or
    /// Silo is quitting.
    pub live: &'a dyn Fn() -> bool,
    /// Names the current step.
    pub announce: &'a dyn Fn(&str),
}

impl Job<'_> {
    fn ensure(&self) -> Result<(), String> {
        if (self.live)() {
            Ok(())
        } else {
            Err(access::CANCELLED.into())
        }
    }
}

/// Installs computer use in the running computer `id` during its setup.
pub(super) fn install(
    app: &AppHandle,
    id: &str,
    approval: computer_use::Approval,
) -> Result<(), String> {
    let live = || ensure_live(id).is_ok();
    let announce = |step: &str| set_detail(app, id, step);
    install_with(
        app,
        id,
        approval,
        &Job {
            live: &live,
            announce: &announce,
        },
    )
}

/// Installs or updates computer use in the running computer `id`. The script is idempotent:
/// it keeps what already is the pinned app and LCU, replaces what is not, writes the same
/// grants again and registers the agents with the current options.
pub(super) fn install_with(
    app: &AppHandle,
    id: &str,
    approval: computer_use::Approval,
    job: &Job,
) -> Result<(), String> {
    let pins = Pins::bundled()?;
    let cache = app_data(app)?.join(CACHE_DIR);
    let lcu_archive = cached(job, &cache, "LCU", pins.lcu_asset())?;
    let (layout, record) = layout_and_record(app, id)?;

    (job.announce)("Copying computer use to the computer");
    job.ensure()?;
    let cancelled = || !(job.live)();
    stage(&layout, &record, &pins, &lcu_archive, &cancelled)?;
    let present = access::run_cancellable(
        &layout,
        &record,
        &format!("/bin/zsh {STAGE}/{SCRIPT_NAME} app-present"),
        APPLY_TIMEOUT,
        &cancelled,
    )?;
    if present.status != 0 {
        // The large download happens only when the computer lacks the pinned app.
        let app_zip = cached(job, &cache, "ChatGPT", pins.app_asset())?;
        (job.announce)("Copying the ChatGPT app to the computer");
        job.ensure()?;
        access::copy_cancellable(
            &layout,
            &record,
            &app_zip,
            &format!(
                "{STAGE}/{}",
                file_name(&pins.app_asset().url).unwrap_or_default()
            ),
            APP_COPY,
            &cancelled,
        )?;
    }

    (job.announce)("Installing computer use");
    job.ensure()?;
    let output = access::run_cancellable(
        &layout,
        &record,
        &format!(
            "/bin/zsh {STAGE}/{SCRIPT_NAME} apply --approval {}",
            approval.as_str()
        ),
        APPLY_TIMEOUT,
        &cancelled,
    );
    // Best effort, also after a cancellation: the archives are large. Only a computer whose
    // access material exists is logged in to; this never creates any.
    if access::credentials_dir(&layout).join("id_ed25519").exists() {
        let _ = access::run(
            &layout,
            &record,
            &format!("rm -rf {STAGE}"),
            None,
            QUICK_COMMAND,
        );
    }
    let output = output?;
    if output.status == 0 {
        Ok(())
    } else {
        Err(failure(&output))
    }
}

/// Whether a started computer's computer use must be updated: its setup is complete and what
/// its guest has is not what this build installs.
pub(super) fn update_wanted(record: &store::Record) -> bool {
    record.installed && record.setup.complete() && record.computer_use_stale()
}

/// After Start number `attempt` of computer `id` is running: updates its computer use on a
/// host thread when it is stale. Returns at once. The update never holds up the user: it
/// ends as soon as the computer is stopped, deleted, restored, forked from or started
/// again, or Silo quits, and a failure only leaves the old version recorded (it is tried
/// again at the next Start) and a line in the computer's log.
pub(super) fn update_in_background(app: &AppHandle, id: &str, attempt: u64) {
    let wanted = registry()
        .entries
        .iter()
        .find(|entry| entry.record.id == id)
        .is_some_and(|entry| update_wanted(&entry.record));
    if !wanted {
        return;
    }
    let Request::Run(claim) = UpdateClaim::request(id, attempt, update_may_run(id, attempt)) else {
        return;
    };
    let (app, id) = (app.clone(), id.to_string());
    let _ = std::thread::Builder::new()
        .name("macos-computer-use-update".into())
        .spawn(move || {
            run_update(&app, &id, attempt);
            while let Some(next) = claim.next_or_release() {
                run_update(&app, &id, next);
            }
        });
}

fn update_may_run(id: &str, attempt: u64) -> bool {
    super::runtime::shutdown::ensure_accepting_operations().is_ok()
        && registry().update_may_run(id, attempt)
}

fn run_update(app: &AppHandle, id: &str, attempt: u64) {
    let live = || update_may_run(id, attempt);
    // Nothing below may touch the computer's folder once the computer is not this Start's.
    if !live() {
        return;
    }
    let log = setup_log::SetupLog::open(app, id);
    let say = |line: &str| {
        if let Some(log) = &log {
            log.line(line);
        }
    };
    let clear = |app: &AppHandle| {
        super::update(app, id, |entry| {
            if entry.attempt == attempt
                && entry
                    .detail
                    .as_deref()
                    .is_some_and(|detail| detail.starts_with(UPDATING))
            {
                entry.detail = None;
            }
        });
    };
    let Ok((layout, record)) = layout_and_record(app, id) else {
        return;
    };
    if !update_wanted(&record) {
        return;
    }
    let approval = record
        .computer_use_approval
        .unwrap_or_else(computer_use::initial_approval);
    let version = templates::computer_use_version_for(approval);
    if access::wait_for_ssh(&layout, &record, UPDATE_SSH_WAIT, &|| !live()).is_err() {
        say("computer use update skipped: the computer did not accept SSH logins while it was running");
        return;
    }
    if !live() {
        return;
    }
    say("computer use update started");
    let announce = |step: &str| {
        super::update(app, id, |entry| {
            if entry.attempt == attempt && entry.state == super::State::Running {
                entry.detail = Some(format!("{UPDATING}: {step}"));
            }
        });
    };
    announce("starting");
    let result = install_with(
        app,
        id,
        approval,
        &Job {
            live: &live,
            announce: &announce,
        },
    );
    match result {
        Ok(()) => {
            let recorded =
                registry().finish_computer_use_update(id, attempt, &version, approval, |record| {
                    store::save(&layout, record)
                });
            match recorded {
                Ok(true) => say("computer use updated"),
                Ok(false) => say(
                    "computer use was updated, but the computer changed meanwhile; it is checked again at the next start",
                ),
                Err(message) => say(&format!(
                    "computer use was updated, but its version could not be saved: {message}"
                )),
            }
        }
        Err(message) if message == access::CANCELLED => {
            say("computer use update cancelled; it runs again at the next start");
        }
        Err(message) => say(&format!(
            "computer use update failed: {message}; it runs again at the next start"
        )),
    }
    clear(app);
}

/// Fails when the setup of computer `id` was cancelled.
fn ensure_live(id: &str) -> Result<(), String> {
    let cancelled = super::registry()
        .entries
        .iter()
        .find(|entry| entry.record.id == id)
        .is_none_or(|entry| entry.cancel.load(Ordering::SeqCst) != super::RUN);
    if cancelled {
        Err(access::CANCELLED.into())
    } else {
        Ok(())
    }
}

/// The script's reason, or the tail of what it printed.
fn failure(output: &CommandOutput) -> String {
    let reason = output
        .stderr
        .lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix("error: "))
        .map(str::to_string)
        .unwrap_or_else(|| {
            let lines: Vec<&str> = output
                .stderr
                .lines()
                .chain(output.stdout.lines())
                .filter(|line| !line.trim().is_empty())
                .collect();
            lines[lines.len().saturating_sub(QUOTE_LINES)..].join(" ")
        });
    format!("Computer use could not be installed in the computer: {reason}")
}

/// Copies the script, the pins and LCU's archive to the guest's staging folder.
fn stage(
    layout: &Layout,
    record: &super::store::Record,
    pins: &Pins,
    lcu_archive: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    let prepared = access::run_cancellable(
        layout,
        record,
        &format!("rm -rf {STAGE} && mkdir -p {STAGE}"),
        QUICK_COMMAND,
        cancelled,
    )?;
    if prepared.status != 0 {
        return Err("Silo could not prepare a folder in the computer.".into());
    }
    let local = tempfile::tempdir()
        .map_err(|error| super::store::io_error("prepare computer use", &error))?;
    let script = local.path().join(SCRIPT_NAME);
    let pinned = local.path().join(PINNED_NAME);
    fs::write(&script, SCRIPT)
        .and_then(|()| fs::write(&pinned, pins.pinned_json()))
        .map_err(|error| super::store::io_error("prepare computer use", &error))?;
    for (file, name) in [(&script, SCRIPT_NAME), (&pinned, PINNED_NAME)] {
        access::copy_cancellable(
            layout,
            record,
            file,
            &format!("{STAGE}/{name}"),
            SMALL_COPY,
            cancelled,
        )?;
    }
    access::copy_cancellable(
        layout,
        record,
        lcu_archive,
        &format!(
            "{STAGE}/{}",
            file_name(&pins.lcu_asset().url).unwrap_or_default()
        ),
        SMALL_COPY,
        cancelled,
    )
}

/// The downloaded, verified file for `asset` in `cache`.
fn cached(job: &Job, cache: &Path, label: &str, asset: &Asset) -> Result<PathBuf, String> {
    let name = file_name(&asset.url).ok_or("Silo's computer use information is invalid.")?;
    let cancelled = || !(job.live)();
    let _turn = loop {
        match DOWNLOAD_TURN.try_lock() {
            Ok(turn) => break turn,
            Err(std::sync::TryLockError::Poisoned(poisoned)) => break poisoned.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                job.ensure()?;
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    };
    for attempt in 0..2 {
        (job.announce)(&format!("Downloading {label}"));
        let path =
            restore_image::download_as(&asset.url, cache, &name, &cancelled, &mut |done, total| {
                let total = total.unwrap_or(asset.bytes).max(1);
                (job.announce)(&format!(
                    "Downloading {label} ({}%)",
                    (done * 100 / total).min(100)
                ));
            })
            .map_err(|error| match error {
                restore_image::DownloadError::Cancelled => access::CANCELLED.to_string(),
                restore_image::DownloadError::Failed(message) => message,
            })?;
        (job.announce)(&format!("Checking {label}"));
        if matches_pin(&path, asset)? {
            return Ok(path);
        }
        let _ = fs::remove_file(&path);
        if attempt == 1 {
            break;
        }
    }
    Err(format!(
        "The {label} download does not match the version Silo expects. Try again later."
    ))
}

/// Whether `path` has the pinned size and SHA-256.
fn matches_pin(path: &Path, asset: &Asset) -> Result<bool, String> {
    let io = |error: std::io::Error| super::store::io_error("check a download", &error);
    let mut file = fs::File::open(path).map_err(io)?;
    if file.metadata().map_err(io)?.len() != asset.bytes {
        return Ok(false);
    }
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let digest: String = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(digest == asset.sha256)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_pins_are_valid_and_agree() {
        let pins = Pins::bundled().unwrap();
        assert_eq!(pins.app.lcu_version, pins.lcu.version);
        assert!(file_name(&pins.app_asset().url)
            .unwrap()
            .contains(&pins.app.version));
        assert!(file_name(&pins.lcu_asset().url)
            .unwrap()
            .contains(&pins.lcu.version));
    }

    #[test]
    fn an_update_is_wanted_for_a_complete_computer_without_the_current_fingerprint() {
        use super::super::store::{CreateRequest, SetupProgress};
        let mut record = store::new_record(
            &CreateRequest {
                name: "mac-one".into(),
                cpus: 4,
                memory_gib: 8,
                disk_gib: 64,
            },
            "02:00:00:00:00:01".into(),
        );
        record.installed = true;
        record.setup = SetupProgress {
            account: true,
            sip: true,
            computer_use: true,
            clipboard: true,
            needs_personalizing: false,
        };
        // A record from before the fingerprint split is stale.
        assert!(update_wanted(&record));
        let approval = computer_use::Approval::Ask;
        record.computer_use_approval = Some(approval);
        record.computer_use_version = Some(templates::computer_use_version_for(approval));
        assert!(!update_wanted(&record));
        record.computer_use_version = Some("old".into());
        assert!(update_wanted(&record));
        // A setup that did not finish is resumed, not updated.
        record.setup.clipboard = false;
        assert!(!update_wanted(&record));
        record.setup.clipboard = true;
        record.installed = false;
        assert!(!update_wanted(&record));
    }

    #[test]
    fn only_one_update_runs_for_a_computer_and_a_later_start_is_queued_behind_it() {
        let run = |id: &str, attempt, owns| UpdateClaim::request(id, attempt, owns);
        let Request::Run(first) = run("update-claim-test", 1, true) else {
            panic!("the first request runs");
        };
        // A second Start's request waits; the highest queued Start wins whatever the
        // order the requests arrive in.
        assert!(matches!(run("update-claim-test", 3, true), Request::Queued));
        assert!(matches!(run("update-claim-test", 2, true), Request::Queued));
        // A request from a Start that no longer owns the computer is refused outright.
        assert!(matches!(run("update-claim-test", 9, false), Request::Stale));
        assert!(matches!(
            run("update-claim-other", 1, true),
            Request::Run(_)
        ));
        assert_eq!(first.next_or_release(), Some(3));
        // With nothing queued the claim is released in the same step.
        assert_eq!(first.next_or_release(), None);
        assert!(matches!(run("update-claim-test", 4, true), Request::Run(_)));
    }

    #[test]
    fn a_finished_claim_never_removes_a_newer_ones_place() {
        let Request::Run(old) = UpdateClaim::request("update-token-test", 1, true) else {
            panic!("runs");
        };
        assert_eq!(old.next_or_release(), None);
        let Request::Run(newer) = UpdateClaim::request("update-token-test", 2, true) else {
            panic!("runs");
        };
        // The old claim's destructor runs after the newer one took the place.
        drop(old);
        assert!(matches!(
            UpdateClaim::request("update-token-test", 3, true),
            Request::Queued
        ));
        assert_eq!(newer.next_or_release(), Some(3));
    }

    #[test]
    fn a_delete_waits_for_the_running_update_to_end() {
        let Request::Run(claim) = UpdateClaim::request("update-drain-test", 1, true) else {
            panic!("runs");
        };
        let waiter = std::thread::spawn(|| wait_for_update_end("update-drain-test"));
        std::thread::sleep(Duration::from_millis(150));
        assert!(!waiter.is_finished());
        drop(claim);
        assert_eq!(waiter.join().unwrap(), Ok(()));
        // Nothing running: it returns at once.
        assert_eq!(wait_for_update_end("update-drain-test"), Ok(()));
    }

    #[test]
    fn a_delete_that_cannot_drain_the_update_fails_and_keeps_the_files() {
        let Request::Run(_claim) = UpdateClaim::request("update-stuck-test", 1, true) else {
            panic!("runs");
        };
        let error = wait_for_update_end_within("update-stuck-test", Duration::from_millis(120))
            .unwrap_err();
        assert!(error.contains("still being updated"));
    }

    #[test]
    fn a_job_that_is_no_longer_live_cancels_the_run() {
        let announced = std::cell::RefCell::new(Vec::new());
        let announce = |step: &str| announced.borrow_mut().push(step.to_string());
        let live = std::cell::Cell::new(true);
        let check = || live.get();
        let job = Job {
            live: &check,
            announce: &announce,
        };
        assert_eq!(job.ensure(), Ok(()));
        live.set(false);
        assert_eq!(job.ensure(), Err(access::CANCELLED.to_string()));
    }

    #[test]
    fn pins_with_a_foreign_host_or_a_bad_digest_are_refused() {
        let wrong_host = APP_LOCK.replace("persistent.oaistatic.com", "example.com");
        assert!(Pins::parse(&wrong_host, LCU_LOCK).is_err());
        let digest = Pins::bundled().unwrap().lcu_asset().sha256.clone();
        let short = LCU_LOCK.replace(&digest, &digest[..63]);
        assert!(Pins::parse(APP_LOCK, &short).is_err());
        let other_lcu = LCU_LOCK.replace("0.11.0", "0.11.1");
        assert!(Pins::parse(APP_LOCK, &other_lcu).is_err());
    }

    #[test]
    fn the_guest_receives_the_pins_the_script_reads() {
        let json: serde_json::Value =
            serde_json::from_str(&Pins::bundled().unwrap().pinned_json()).unwrap();
        for key in [
            "chatgptVersion",
            "chatgptArchive",
            "chatgptSha256",
            "bundleIdentifier",
            "teamIdentifier",
            "lcuVersion",
            "lcuArchive",
            "lcuSha256",
        ] {
            assert!(json[key].is_string(), "{key}");
            assert!(SCRIPT.contains(&format!("pinned {key}")), "{key}");
        }
    }

    #[test]
    fn file_names_are_plain() {
        assert_eq!(file_name("https://h/a/b-1.zip").as_deref(), Some("b-1.zip"));
        assert_eq!(file_name("https://h/a/"), None);
        assert_eq!(file_name("https://h/a/.hidden"), None);
        assert_eq!(file_name("https://h/a/x%20y"), None);
    }

    #[test]
    fn a_download_must_have_the_pinned_size_and_digest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file");
        fs::write(&path, b"abc").unwrap();
        let asset = |bytes, sha256: &str| Asset {
            url: "https://h/file".into(),
            sha256: sha256.into(),
            bytes,
        };
        let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(matches_pin(&path, &asset(3, abc)).unwrap());
        assert!(!matches_pin(&path, &asset(4, abc)).unwrap());
        assert!(!matches_pin(&path, &asset(3, &"0".repeat(64))).unwrap());
    }

    #[test]
    fn a_failure_quotes_the_scripts_reason() {
        let output = CommandOutput {
            status: 1,
            stdout: "Installing ChatGPT\n".into(),
            stderr: "noise\nerror: lcu setup failed\n".into(),
        };
        assert_eq!(
            failure(&output),
            "Computer use could not be installed in the computer: lcu setup failed"
        );
        let silent = CommandOutput {
            status: 1,
            stdout: "a\nb\nc\nd\n".into(),
            stderr: String::new(),
        };
        assert!(failure(&silent).ends_with("b c d"));
    }

    #[cfg(target_os = "macos")]
    mod script {
        use std::process::Command;

        const SCRIPT_PATH: &str = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/guest/macos/silo-computer-use.zsh"
        );

        #[test]
        fn the_script_parses() {
            let output = Command::new("/bin/zsh")
                .args(["-n", SCRIPT_PATH])
                .output()
                .unwrap();
            assert!(output.status.success(), "{:?}", output.stderr);
        }

        /// An `access` table as macOS 14 and later create it, plus a column this version of
        /// the script does not know.
        const SCHEMA: &str = "CREATE TABLE access (service TEXT NOT NULL, client TEXT NOT NULL, \
            client_type INTEGER NOT NULL, auth_value INTEGER NOT NULL, auth_reason INTEGER NOT NULL, \
            auth_version INTEGER NOT NULL, csreq BLOB, policy_id INTEGER, \
            indirect_object_identifier_type INTEGER, indirect_object_identifier TEXT NOT NULL DEFAULT 'UNUSED', \
            indirect_object_code_identity BLOB, flags INTEGER, last_modified INTEGER NOT NULL DEFAULT (CAST(strftime('%s','now') AS INTEGER)), \
            pid INTEGER, pid_version INTEGER, boot_uuid TEXT NOT NULL DEFAULT 'UNUSED', last_reminded INTEGER NOT NULL DEFAULT 0, \
            PRIMARY KEY (service, client, client_type, indirect_object_identifier));";

        fn run(
            db: &std::path::Path,
            home: &std::path::Path,
            args: &[&str],
        ) -> std::process::Output {
            Command::new("/bin/zsh")
                .arg(SCRIPT_PATH)
                .args(args)
                .env("HOME", home)
                .env("SILO_CU_TCC_DB", db)
                .env("SILO_CU_SUDO", "")
                .env("SILO_CU_WORK", home.join("work"))
                .output()
                .unwrap()
        }

        #[test]
        fn a_grant_writes_the_columns_the_table_has() {
            let dir = tempfile::tempdir().unwrap();
            let db = dir.path().join("TCC.db");
            let sqlite = |sql: &str| {
                let output = Command::new("/usr/bin/sqlite3")
                    .arg(&db)
                    .arg(sql)
                    .output()
                    .unwrap();
                assert!(output.status.success(), "{:?}", output.stderr);
                String::from_utf8(output.stdout).unwrap()
            };
            sqlite(SCHEMA);
            let output = run(
                &db,
                dir.path(),
                &[
                    "grant",
                    "kTCCServiceAccessibility",
                    "com.apple.calculator",
                    "/System/Applications/Calculator.app",
                ],
            );
            assert!(output.status.success(), "{:?}", output);
            // A second run replaces the row.
            assert!(run(
                &db,
                dir.path(),
                &[
                    "grant",
                    "kTCCServiceAccessibility",
                    "com.apple.calculator",
                    "/System/Applications/Calculator.app",
                ],
            )
            .status
            .success());
            assert_eq!(
                sqlite(
                    "select service, client, client_type, auth_value, auth_reason, auth_version, \
                     indirect_object_identifier, length(csreq) > 0, last_modified > 0 from access"
                )
                .trim(),
                "kTCCServiceAccessibility|com.apple.calculator|0|2|4|1|UNUSED|1|1"
            );
        }

        #[test]
        fn screen_sharing_gets_screen_capture_accessibility_and_post_event() {
            let home = tempfile::tempdir().unwrap();
            let output = zsh(&["sharing-grants"], "", home.path());
            assert!(output.status.success());
            let rows: Vec<String> = String::from_utf8(output.stdout)
                .unwrap()
                .lines()
                .map(str::to_string)
                .collect();
            let mut expected = Vec::new();
            for client in [
                "com.apple.screensharing.agent",
                "com.apple.screensharing.daemon",
            ] {
                for service in [
                    "kTCCServiceScreenCapture",
                    "kTCCServiceAccessibility",
                    "kTCCServicePostEvent",
                ] {
                    expected.push(format!("{service} {client}"));
                }
            }
            assert_eq!(rows, expected);
            assert!(super::super::SCRIPT.contains("ScreensharingAgent.bundle"));
            assert!(super::super::SCRIPT.contains("screensharingd.bundle"));
            assert!(!super::super::SCRIPT.contains("setvncpw"));
        }

        fn zsh(args: &[&str], stdin: &str, home: &std::path::Path) -> std::process::Output {
            use std::io::Write;
            let mut child = Command::new("/bin/zsh")
                .arg(SCRIPT_PATH)
                .args(args)
                .env("HOME", home)
                .env("SILO_CU_WORK", home.join("work"))
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(stdin.as_bytes())
                .unwrap();
            child.wait_with_output().unwrap()
        }

        #[test]
        fn archive_members_must_stay_relative() {
            let home = tempfile::tempdir().unwrap();
            let ok = |names: &str| zsh(&["safe-members"], names, home.path()).status.success();
            assert!(ok(
                "ChatGPT.app/\nChatGPT.app/Contents/Info.plist\nlcu/bin/..x\n"
            ));
            assert!(!ok("/etc/passwd\n"));
            assert!(!ok("ChatGPT.app/../../x\n"));
            assert!(!ok("../x\n"));
            assert!(!ok("a/..\n"));
            assert!(!ok("..\n"));
        }

        #[test]
        fn links_must_stay_inside_the_extracted_folder() {
            let home = tempfile::tempdir().unwrap();
            let root = home.path().join("tree");
            std::fs::create_dir_all(root.join("sub")).unwrap();
            std::fs::write(root.join("file"), b"x").unwrap();
            std::os::unix::fs::symlink("../file", root.join("sub/inside")).unwrap();
            let check = |root: &std::path::Path| {
                zsh(&["links-inside", root.to_str().unwrap()], "", home.path())
                    .status
                    .success()
            };
            assert!(check(&root));
            std::os::unix::fs::symlink("/etc", root.join("sub/outside")).unwrap();
            assert!(!check(&root));
            std::fs::remove_file(root.join("sub/outside")).unwrap();
            std::os::unix::fs::symlink("/nonexistent-target", root.join("sub/dangling")).unwrap();
            assert!(!check(&root));
            std::fs::remove_file(root.join("sub/dangling")).unwrap();
            // LCU's `node` link points through `app`, which only exists after installation.
            std::os::unix::fs::symlink("../app/node", root.join("sub/later")).unwrap();
            assert!(check(&root));
            std::os::unix::fs::symlink("../../missing", root.join("sub/escape")).unwrap();
            assert!(!check(&root));
            std::fs::remove_file(root.join("sub/escape")).unwrap();
            std::os::unix::fs::symlink("../..", root.join("sub/parent")).unwrap();
            assert!(!check(&root.join("sub")));
        }

        #[test]
        fn a_table_without_the_needed_columns_is_refused() {
            let dir = tempfile::tempdir().unwrap();
            let db = dir.path().join("TCC.db");
            let created = Command::new("/usr/bin/sqlite3")
                .arg(&db)
                .arg("create table access (service text, client text);")
                .output()
                .unwrap();
            assert!(created.status.success());
            let output = run(
                &db,
                dir.path(),
                &[
                    "grant",
                    "kTCCServiceAccessibility",
                    "com.apple.calculator",
                    "/System/Applications/Calculator.app",
                ],
            );
            assert!(!output.status.success());
        }
    }
}
