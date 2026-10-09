//! Computer use inside the guest: the official ChatGPT macOS app and LCU's macOS build,
//! with the Accessibility and Screen Recording grants they need.
//!
//! The host downloads the two pinned archives (`guest/macos/*-lock.json`) into its own cache,
//! checks their SHA-256, copies them with `guest/macos/silo-computer-use.zsh` into the running
//! computer and runs the script there as `silo`. The script does the installation, writes the
//! TCC rows and registers LCU's agents; see its header for the steps.
use super::guest_access::{self as access, CommandOutput};
use super::store::Layout;
use super::{app_data, layout_and_record, restore_image, set_detail};
use crate::computer_use;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
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

/// Installs computer use in the running computer `id`.
pub(super) fn install(app: &AppHandle, id: &str) -> Result<(), String> {
    let pins = Pins::bundled()?;
    let approval = computer_use::initial_approval();
    let cache = app_data(app)?.join(CACHE_DIR);
    let app_zip = cached(app, id, &cache, "ChatGPT", pins.app_asset())?;
    let lcu_archive = cached(app, id, &cache, "LCU", pins.lcu_asset())?;
    let (layout, record) = layout_and_record(app, id)?;

    set_detail(app, id, "Copying computer use to the computer");
    ensure_live(id)?;
    stage(&layout, &record, &pins, &lcu_archive)?;
    let present = access::run(
        &layout,
        &record,
        &format!("/bin/zsh {STAGE}/{SCRIPT_NAME} app-present"),
        None,
        APPLY_TIMEOUT,
    )?;
    if present.status != 0 {
        set_detail(app, id, "Copying the ChatGPT app to the computer");
        ensure_live(id)?;
        access::copy(
            &layout,
            &record,
            &app_zip,
            &format!(
                "{STAGE}/{}",
                file_name(&pins.app_asset().url).unwrap_or_default()
            ),
            APP_COPY,
        )?;
    }

    set_detail(app, id, "Installing computer use");
    ensure_live(id)?;
    let output = access::run(
        &layout,
        &record,
        &format!(
            "/bin/zsh {STAGE}/{SCRIPT_NAME} apply --approval {}",
            approval.as_str()
        ),
        None,
        APPLY_TIMEOUT,
    )?;
    // Best effort: the archives are large and the script removes them on success.
    let _ = access::run(
        &layout,
        &record,
        &format!("rm -rf {STAGE}"),
        None,
        QUICK_COMMAND,
    );
    if output.status == 0 {
        Ok(())
    } else {
        Err(failure(&output))
    }
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
) -> Result<(), String> {
    let prepared = access::run(
        layout,
        record,
        &format!("rm -rf {STAGE} && mkdir -p {STAGE}"),
        None,
        QUICK_COMMAND,
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
        access::copy(layout, record, file, &format!("{STAGE}/{name}"), SMALL_COPY)?;
    }
    access::copy(
        layout,
        record,
        lcu_archive,
        &format!(
            "{STAGE}/{}",
            file_name(&pins.lcu_asset().url).unwrap_or_default()
        ),
        SMALL_COPY,
    )
}

/// The downloaded, verified file for `asset` in `cache`.
fn cached(
    app: &AppHandle,
    id: &str,
    cache: &Path,
    label: &str,
    asset: &Asset,
) -> Result<PathBuf, String> {
    let name = file_name(&asset.url).ok_or("Silo's computer use information is invalid.")?;
    let _turn = DOWNLOAD_TURN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let cancelled = || ensure_live(id).is_err();
    for attempt in 0..2 {
        set_detail(app, id, &format!("Downloading {label}"));
        let path =
            restore_image::download_as(&asset.url, cache, &name, &cancelled, &mut |done, total| {
                let total = total.unwrap_or(asset.bytes).max(1);
                set_detail(
                    app,
                    id,
                    &format!("Downloading {label} ({}%)", (done * 100 / total).min(100)),
                );
            })
            .map_err(|error| match error {
                restore_image::DownloadError::Cancelled => access::CANCELLED.to_string(),
                restore_image::DownloadError::Failed(message) => message,
            })?;
        set_detail(app, id, &format!("Checking {label}"));
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
    fn pins_with_a_foreign_host_or_a_bad_digest_are_refused() {
        let wrong_host = APP_LOCK.replace("persistent.oaistatic.com", "example.com");
        assert!(Pins::parse(&wrong_host, LCU_LOCK).is_err());
        let digest = Pins::bundled().unwrap().lcu_asset().sha256.clone();
        let short = LCU_LOCK.replace(&digest, &digest[..63]);
        assert!(Pins::parse(APP_LOCK, &short).is_err());
        let other_lcu = LCU_LOCK.replace("0.10.1", "0.10.2");
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
