//! Installs verified Apple Command Line Tools, Git LFS and GitHub CLI in a macOS computer.
use super::{guest_access, layout_and_record, MacosContext};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, TryLockError},
    time::Duration,
};

const SCRIPT: &str = include_str!("../../../guest/macos/silo-developer-tools.zsh");
const PIN: &str = include_str!("../../../guest/macos/developer-tools-lock.json");
const GITHUB_SETUP: &str = include_str!("../../../guest/setup-github.sh");
const CACHE_DIR: &str = "macos-developer-tools";
const STAGE: &str = "/private/tmp/silo-developer-tools";
const INSTALL_TIMEOUT: Duration = Duration::from_secs(60 * 60);
const COPY_TIMEOUT: Duration = Duration::from_secs(60 * 60);
static DOWNLOAD_TURN: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Asset {
    file: String,
    url: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppleSigner {
    identity: String,
    leaf_certificate_sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CltPackage {
    #[serde(flatten)]
    asset: Asset,
    receipt_id: String,
    receipt_version: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CltPin {
    version: String,
    packages: Vec<CltPackage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Lock {
    schema_version: u32,
    supported_guest_builds: Vec<String>,
    apple_signer: AppleSigner,
    clt: CltPin,
    git_lfs: PinnedTool,
    gh: PinnedTool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PinnedTool {
    version: String,
    #[serde(flatten)]
    asset: Asset,
}

fn valid_version(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
}

fn valid_file(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('.')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn validate_asset(asset: &Asset, host: &str, extension: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(&asset.url) else {
        return false;
    };
    valid_file(&asset.file)
        && asset.file.ends_with(extension)
        && asset.bytes > 0
        && asset.sha256.len() == 64
        && asset
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        && url.scheme() == "https"
        && url.host_str() == Some(host)
        && url.path().ends_with(&format!("/{}", asset.file))
}

fn lock() -> Result<Lock, String> {
    let invalid = || "Silo's developer tools lock is invalid.".to_string();
    let lock: Lock = serde_json::from_str(PIN).map_err(|_| invalid())?;
    let required_receipts = [
        "com.apple.pkg.CLTools_Executables",
        "com.apple.pkg.CLTools_SDK_macOS_NMOS",
        "com.apple.pkg.CLTools_SDK_macOS_LMOS",
        "com.apple.pkg.CLTools_macOS_SDK",
        "com.apple.pkg.CLTools_SwiftBackDeploy",
    ];
    let receipt_ids: Vec<&str> = lock
        .clt
        .packages
        .iter()
        .map(|package| package.receipt_id.as_str())
        .collect();
    if lock.schema_version != 1
        || lock.supported_guest_builds.is_empty()
        || lock.supported_guest_builds.iter().any(|build| {
            build.is_empty()
                || !build
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.')
        })
        || lock.apple_signer.identity != "Software Update"
        || lock.apple_signer.leaf_certificate_sha256.len() != 64
        || !lock
            .apple_signer
            .leaf_certificate_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || !valid_version(&lock.clt.version)
        || lock.clt.packages.len() != required_receipts.len()
        || required_receipts
            .iter()
            .any(|required| !receipt_ids.contains(required))
        || receipt_ids
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != receipt_ids.len()
        || lock.clt.packages.iter().any(|package| {
            !validate_asset(&package.asset, "swcdn.apple.com", ".pkg")
                || !valid_version(&package.receipt_version)
                || package
                    .receipt_id
                    .bytes()
                    .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_')))
        })
        || !valid_version(&lock.git_lfs.version)
        || !validate_asset(&lock.git_lfs.asset, "github.com", ".zip")
        || lock.git_lfs.asset.file != format!("git-lfs-darwin-arm64-v{}.zip", lock.git_lfs.version)
        || !lock
            .git_lfs
            .asset
            .url
            .contains("/git-lfs/git-lfs/releases/download/")
        || !valid_version(&lock.gh.version)
        || !validate_asset(&lock.gh.asset, "github.com", ".zip")
        || lock.gh.asset.file != format!("gh_{}_macOS_arm64.zip", lock.gh.version)
        || !lock.gh.asset.url.contains("/cli/cli/releases/download/")
    {
        return Err(invalid());
    }
    Ok(lock)
}

pub(crate) fn supports_guest_build(build: &str) -> Result<bool, String> {
    Ok(lock()?
        .supported_guest_builds
        .iter()
        .any(|supported| supported == build))
}

/// Inputs that make the developer tools step and template fingerprint change together.
pub(super) fn setup_inputs() -> [&'static str; 3] {
    [SCRIPT, PIN, GITHUB_SETUP]
}

/// Fingerprint for the installed developer tools on a computer.
pub(super) fn version() -> String {
    let mut hash = Sha256::new();
    for input in setup_inputs() {
        hash.update((input.len() as u64).to_le_bytes());
        hash.update(input.as_bytes());
    }
    hash.finalize()[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn script(lock: &Lock) -> String {
    let packages = lock
        .clt
        .packages
        .iter()
        .map(|package| {
            format!(
                "  '{}|{}|{}|{}|{}'",
                package.asset.file,
                package.receipt_id,
                package.receipt_version,
                package.asset.bytes,
                package.asset.sha256
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let supported_builds = lock
        .supported_guest_builds
        .iter()
        .map(|build| format!("'{build}'"))
        .collect::<Vec<_>>()
        .join(" ");
    let executable_receipt_version = lock
        .clt
        .packages
        .iter()
        .find(|package| package.receipt_id == "com.apple.pkg.CLTools_Executables")
        .map_or("".to_string(), |package| package.receipt_version.clone());
    let lock_sha256 = digest(PIN.as_bytes());
    let tools_version = format!(
        "{}-lfs-{}-gh-{}",
        lock.clt.version, lock.git_lfs.version, lock.gh.version
    );
    SCRIPT
        .replace("@TOOLS_VERSION@", &tools_version)
        .replace("@SUPPORTED_GUEST_BUILDS@", &supported_builds)
        .replace("@APPLE_SIGNER@", &lock.apple_signer.identity)
        .replace(
            "@APPLE_SIGNER_SHA256@",
            &lock.apple_signer.leaf_certificate_sha256,
        )
        .replace("@CLT_VERSION@", &lock.clt.version)
        .replace("@CLT_PACKAGES@", &packages)
        .replace(
            "@CLT_EXECUTABLES_RECEIPT_VERSION@",
            &executable_receipt_version,
        )
        .replace("@GIT_LFS_VERSION@", &lock.git_lfs.version)
        .replace("@GIT_LFS_FILE@", &lock.git_lfs.asset.file)
        .replace("@GIT_LFS_BYTES@", &lock.git_lfs.asset.bytes.to_string())
        .replace("@GIT_LFS_SHA256@", &lock.git_lfs.asset.sha256)
        .replace("@GH_VERSION@", &lock.gh.version)
        .replace("@GH_FILE@", &lock.gh.asset.file)
        .replace("@GH_BYTES@", &lock.gh.asset.bytes.to_string())
        .replace("@GH_SHA256@", &lock.gh.asset.sha256)
        .replace("@LOCK_SHA256@", &lock_sha256)
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn hash_file(path: &Path) -> Result<String, String> {
    let io =
        |error: std::io::Error| super::store::io_error("check a developer tools download", &error);
    let mut file = fs::File::open(path).map_err(io)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn cached(
    app: &MacosContext,
    id: &str,
    cache: &Path,
    asset: &Asset,
    cancelled: &dyn Fn() -> bool,
) -> Result<PathBuf, String> {
    for attempt in 0..2 {
        let path = super::restore_image::download_as(
            &asset.url,
            cache,
            &asset.file,
            cancelled,
            &mut |done, total| {
                let total = total.unwrap_or(asset.bytes).max(1);
                super::set_detail(
                    app,
                    id,
                    &format!(
                        "Downloading {} ({}%)",
                        asset.file,
                        (done.saturating_mul(100) / total).min(100)
                    ),
                );
            },
        )
        .map_err(|error| match error {
            super::restore_image::DownloadError::Cancelled => guest_access::CANCELLED.to_string(),
            super::restore_image::DownloadError::Failed(message) => message,
        })?;
        let matches = fs::metadata(&path)
            .map(|metadata| metadata.len() == asset.bytes)
            .unwrap_or(false)
            && hash_file(&path)? == asset.sha256;
        if matches {
            return Ok(path);
        }
        let _ = fs::remove_file(&path);
        if attempt == 1 {
            break;
        }
    }
    Err(format!(
        "The {} download does not match the version Silo expects. Try again later.",
        asset.file
    ))
}

fn download_turn(cancelled: &dyn Fn() -> bool) -> Result<MutexGuard<'static, ()>, String> {
    loop {
        match DOWNLOAD_TURN.try_lock() {
            Ok(turn) => return Ok(turn),
            Err(TryLockError::Poisoned(poisoned)) => return Ok(poisoned.into_inner()),
            Err(TryLockError::WouldBlock) => {
                if cancelled() {
                    return Err(guest_access::CANCELLED.into());
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    }
}

/// Downloads, verifies and stages locked artifacts on the host, then installs them unattended.
pub(super) fn install(
    app: &MacosContext,
    id: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    let lock = lock()?;
    let (layout, record) = layout_and_record(app, id)?;
    let guest_build = record
        .restore_image
        .as_ref()
        .map(|image| image.build.as_str())
        .ok_or_else(|| "This computer has no known macOS build for developer tools.".to_string())?;
    if !lock
        .supported_guest_builds
        .iter()
        .any(|supported| supported == guest_build)
    {
        return Err(format!(
            "Developer tools are not locked for macOS build {guest_build}. Try setup again after Silo updates its developer tools lock."
        ));
    }
    let operation_cancelled = || cancelled() || crate::runtime::OPERATIONS.removal_queued(id);
    let cache = super::app_data(app)?.join(CACHE_DIR);
    let _turn = download_turn(&operation_cancelled)?;
    let mut artifacts = Vec::with_capacity(lock.clt.packages.len() + 2);
    for package in &lock.clt.packages {
        artifacts.push(cached(
            app,
            id,
            &cache,
            &package.asset,
            &operation_cancelled,
        )?);
    }
    artifacts.push(cached(
        app,
        id,
        &cache,
        &lock.git_lfs.asset,
        &operation_cancelled,
    )?);
    artifacts.push(cached(
        app,
        id,
        &cache,
        &lock.gh.asset,
        &operation_cancelled,
    )?);
    if operation_cancelled() {
        return Err(guest_access::CANCELLED.into());
    }

    let local = tempfile::tempdir()
        .map_err(|error| super::store::io_error("prepare developer tools", &error))?;
    let script_path = local.path().join("silo-developer-tools.zsh");
    let lock_path = local.path().join("developer-tools-lock.json");
    let github_setup_path = local.path().join("setup-github.sh");
    fs::write(&script_path, script(&lock))
        .and_then(|()| fs::write(&lock_path, PIN))
        .and_then(|()| fs::write(&github_setup_path, GITHUB_SETUP))
        .map_err(|error| super::store::io_error("prepare developer tools", &error))?;
    let ready = guest_access::run_cancellable(
        &layout,
        &record,
        &format!("/usr/bin/sudo -n /bin/rm -rf {STAGE} && /bin/mkdir -p {STAGE}"),
        Duration::from_secs(60),
        &operation_cancelled,
    )?;
    if ready.status != 0 {
        return Err("Silo could not prepare the developer tools staging folder.".into());
    }
    for (file, name) in [
        (&script_path, "silo-developer-tools.zsh"),
        (&lock_path, "developer-tools-lock.json"),
        (&github_setup_path, "setup-github.sh"),
    ] {
        if operation_cancelled() {
            return Err(guest_access::CANCELLED.into());
        }
        guest_access::copy_cancellable(
            &layout,
            &record,
            file,
            &format!("{STAGE}/{name}"),
            COPY_TIMEOUT,
            &operation_cancelled,
        )?;
    }
    for path in &artifacts {
        if operation_cancelled() {
            return Err(guest_access::CANCELLED.into());
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "Silo's developer tools download has an invalid name.".to_string())?;
        super::set_detail(app, id, &format!("Copying {name} to the computer"));
        guest_access::copy_cancellable(
            &layout,
            &record,
            path,
            &format!("{STAGE}/{name}"),
            COPY_TIMEOUT,
            &operation_cancelled,
        )?;
    }
    super::set_detail(app, id, "Installing developer tools");
    let script = script(&lock);
    let output = guest_access::run_cancellable_with_input(
        &layout,
        &record,
        "/bin/zsh -s",
        script.as_bytes(),
        INSTALL_TIMEOUT,
        &operation_cancelled,
    )?;
    if output.status == 0 {
        return Ok(());
    }
    let reason = failure_reason(&output.stderr, &output.stdout);
    Err(format!("Developer tools could not be installed: {reason}"))
}

fn failure_reason<'a>(stderr: &'a str, stdout: &'a str) -> &'a str {
    stderr
        .lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix("error: "))
        .filter(|line| !line.trim().is_empty())
        .or_else(|| stderr.lines().rev().find(|line| !line.trim().is_empty()))
        .or_else(|| stdout.lines().rev().find(|line| !line.trim().is_empty()))
        .unwrap_or("the installer returned an error")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::unix::fs::PermissionsExt, process::Command};

    #[test]
    fn checked_lock_pins_apple_packages_lfs_and_gh() {
        let lock = lock().unwrap();
        assert_eq!(lock.supported_guest_builds, ["25G83"]);
        assert_eq!(lock.apple_signer.identity, "Software Update");
        assert_eq!(lock.clt.version, "26.6");
        assert_eq!(lock.clt.packages.len(), 5);
        assert_eq!(lock.git_lfs.version, "3.8.0");
        assert_eq!(lock.gh.version, "2.102.0");
        assert_eq!(lock.git_lfs.asset.bytes, 5_550_634);
        assert_eq!(lock.gh.asset.bytes, 14_359_983);
    }

    #[test]
    fn supported_guest_build_check_uses_the_validated_lock() {
        assert!(supports_guest_build("25G83").unwrap());
        assert!(!supports_guest_build("24A123").unwrap());
    }

    #[test]
    fn setup_fingerprint_covers_installer_lock_and_shared_github_setup() {
        let inputs = setup_inputs();
        assert!(inputs[0].contains("installer -pkg"));
        assert!(inputs[1].contains("supportedGuestBuilds"));
        assert!(inputs[2].contains("silo-github-credential"));
        assert_eq!(version().len(), 16);
    }

    #[test]
    fn root_runs_the_shared_github_setup_with_its_own_home() {
        // macOS sudo keeps HOME. Without this, `gh --version` run by root left a root-owned
        // ~/.local in the account home and the computer-use install failed with EACCES
        // (live qualification, 2026-10-14).
        let call = SCRIPT
            .lines()
            .find(|line| line.contains("setup-github.sh\"") && line.contains("sudo"))
            .expect("the installer runs the shared GitHub setup through sudo");
        assert!(call.contains("\"HOME=/var/root\""), "{call}");
    }

    #[test]
    fn installer_expands_a_checked_lock_without_unresolved_placeholders() {
        let script = script(&lock().unwrap());
        assert!(script.contains("CLTools_Executables_Universal.pkg"));
        assert!(script.contains("git-lfs-darwin-arm64-v3.8.0.zip"));
        assert!(script.contains("gh_2.102.0_macOS_arm64.zip"));
        assert!(script.contains("25G83"));
        assert!(script.contains("E074D204AC2498E9DC904A7BC7CED8464119B79D05668028920583B1E896EBB4"));
        assert!(script.contains("developer-tools-receipt.json"));
        assert!(!script.contains("@CLT_"));
        assert!(!script.contains("@GIT_LFS_"));
        assert!(!script.contains("@GH_"));
        assert!(!script.contains("@LOCK_SHA256@"));
    }

    #[test]
    fn matching_clt_receipt_does_not_skip_reinstall_when_payload_is_missing() {
        const BEGIN: &str = "# BEGIN CLT PAYLOAD HEALTH HELPERS";
        const END: &str = "# END CLT PAYLOAD HEALTH HELPERS";
        let start = SCRIPT
            .find(BEGIN)
            .expect("installer must define its CLT payload health check");
        let helpers = &SCRIPT[start..];
        let end = helpers
            .find(END)
            .expect("CLT payload health helpers must have a clear test seam");
        let helpers = &helpers[..end];

        let root = tempfile::tempdir().unwrap();
        let developer_directory = root.path().join("CommandLineTools");
        let work = root.path().join("work");
        let bin = developer_directory.join("usr/bin");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(developer_directory.join("SDKs/MacOSX.sdk")).unwrap();
        fs::create_dir_all(&work).unwrap();

        let git = bin.join("git");
        fs::write(&git, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();
        let clang = bin.join("clang");
        fs::write(
            &clang,
            "#!/bin/sh\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = \"-o\" ]; then\n    shift\n    printf '#!/bin/sh\\nexit 0\\n' >\"$1\"\n    chmod +x \"$1\"\n    exit 0\n  fi\n  shift\ndone\nexit 1\n",
        )
        .unwrap();
        fs::set_permissions(&clang, fs::Permissions::from_mode(0o755)).unwrap();

        let quote = |path: &Path| format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"));
        let run_decision = || {
            let source = format!(
                "{helpers}\nclt_repair=0\nif clt_needs_reinstall '26.6.0.0.1781586589' '26.6.0.0.1781586589' {} {}; then clt_repair=1; fi\nif clt_package_needs_install '26.6.0.0.1781586589' '26.6.0.0.1781586589' \"$clt_repair\"; then exit 0; else exit 1; fi\n",
                quote(&developer_directory),
                quote(&work)
            );
            Command::new("/bin/zsh")
                .args(["-c", &source])
                .output()
                .expect("zsh is available on the supported macOS host")
        };

        // A matching package receipt cannot make a missing required compiler look ready.
        let missing = run_decision();
        assert!(
            missing.status.success(),
            "missing CLT payload must request reinstall: {}",
            String::from_utf8_lossy(&missing.stderr)
        );

        let swiftc = bin.join("swiftc");
        fs::write(&swiftc, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&swiftc, fs::Permissions::from_mode(0o755)).unwrap();
        let healthy = run_decision();
        assert!(
            !healthy.status.success(),
            "matching receipts with usable CLT payload should skip reinstall: {}",
            String::from_utf8_lossy(&healthy.stderr)
        );
    }

    #[test]
    fn asset_validation_requires_https_and_the_pinned_release_host() {
        let lock = lock().unwrap();
        let mut asset = lock.git_lfs.asset;
        assert!(validate_asset(&asset, "github.com", ".zip"));
        asset.url = asset.url.replace("https://", "http://");
        assert!(!validate_asset(&asset, "github.com", ".zip"));
    }

    #[test]
    fn install_failure_prefers_the_guest_error_over_stdout_progress() {
        assert_eq!(
            failure_reason(
                "log noise\nerror: receipt verification failed\n",
                "Verified package\n"
            ),
            "receipt verification failed"
        );
        assert_eq!(
            failure_reason("installer failed\n", "Installing tools\n"),
            "installer failed"
        );
        assert_eq!(failure_reason("", ""), "the installer returned an error");
    }
}
