use serde::{Deserialize, Serialize};
#[cfg(any(target_os = "linux", test))]
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::fs;
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager};

const REINSTALL_GUIDANCE: &str = "Reinstall Silo from its original download or package manager, keeping its app data. Your computers and settings are stored separately from the app.";
const RETRY_GUIDANCE: &str =
    "Retry checks. If this keeps happening, quit and reopen Silo, then retry.";

// Finish native probes before the frontend watchdog abandons the report at 15 seconds.
/// Each group of checks gets its own budget so one slow check cannot starve the rest.
const SYSTEM_BUDGET: Duration = Duration::from_secs(4);
const MICROSANDBOX_BUDGET: Duration = Duration::from_secs(15);
const GIT_BUDGET: Duration = Duration::from_secs(10);
const PROCESS_TIMEOUT: Duration = Duration::from_secs(3);
#[cfg(any(target_os = "linux", test))]
const MAX_HASH_BYTES: u64 = 128 * 1024 * 1024;
const MAX_OUTPUT: u64 = 8 * 1024;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
// Embed reviewed source inputs; never derive approval from staged package metadata.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeInputs {
    schema_version: u8,
    microsandbox_version: String,
    libkrunfw_version: String,
    source_commit: String,
    source_archive_sha256: String,
    patches: Vec<RuntimePatchInput>,
    toolchain: String,
    features: String,
    targets: std::collections::HashMap<String, RuntimeTarget>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimePatchInput {
    path: String,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeTarget {
    executable_asset: String,
    executable_sha256: String,
    agentd_asset: String,
    agentd_sha256: String,
    library_name: String,
    library_asset: String,
    library_sha256: String,
}

static RUNTIME_INPUTS: std::sync::LazyLock<RuntimeInputs> = std::sync::LazyLock::new(|| {
    let inputs: RuntimeInputs = serde_json::from_str(include_str!("../../runtime-inputs.json"))
        .expect("checked-in runtime inputs must be valid");
    assert_eq!(inputs.schema_version, 2, "unsupported runtime input schema");
    inputs
});
const EXPECTED_GIT: &str = "2.53.0";
const EXPECTED_GIT_LFS: &str = "3.7.1";

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DependencyReport {
    schema_version: u8,
    request_id: String,
    checked_at_ms: u64,
    checks: Vec<DependencyCheck>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DependencyCheck {
    id: String,
    title: String,
    status: CheckStatus,
    detail: String,
    remediation: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum CheckStatus {
    Pass,
    Failed,
    Unavailable,
    Timeout,
}

impl DependencyCheck {
    fn pass(id: &str, title: &str, detail: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            status: CheckStatus::Pass,
            detail: detail.into(),
            remediation: None,
        }
    }

    fn failure(
        id: &str,
        title: &str,
        status: CheckStatus,
        detail: impl Into<String>,
        remediation: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            status,
            detail: detail.into(),
            remediation: Some(remediation.into()),
        }
    }
}

#[derive(Debug)]
enum ProbeError {
    Missing(String),
    Unreadable(String),
    Timeout,
    Malformed(String),
    Unsupported(String),
    Unavailable(String),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MicrosandboxManifest {
    schema_version: u8,
    microsandbox_version: String,
    libkrunfw_version: String,
    target_triple: String,
    executable: PatchedRuntimeFile,
    library: RuntimeFile,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RuntimeFile {
    bundled_name: String,
    release_asset: String,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PatchedRuntimeFile {
    bundled_name: String,
    sha256: String,
    source_commit: String,
    source_archive_sha256: String,
    patch_sha256s: Vec<String>,
    toolchain: String,
    features: String,
    official_release_asset: String,
    official_release_sha256: String,
    embedded_agentd_release_asset: String,
    embedded_agentd_release_sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GitManifest {
    schema_version: u8,
    target_triple: String,
    distribution: String,
    distribution_release: String,
    distribution_commit: String,
    git_version: String,
    git_version_output: String,
    git_lfs_version: String,
    archive: GitArchive,
    minimum_platform: String,
    paths: GitPaths,
    executable_sha256: GitExecutableHashes,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GitArchive {
    name: String,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GitPaths {
    git: String,
    git_exec_path: String,
    git_lfs: String,
    templates: String,
    certificate_bundle: Option<String>,
    packaged_executables: GitExecutables,
    packaged_resource_directory: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GitExecutables {
    directory: String,
    git: String,
    git_lfs: String,
    git_remote_http: String,
    git_remote_https: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GitExecutableHashes {
    git: String,
    git_lfs: String,
    git_remote_http: String,
    git_remote_https: String,
}

struct ProbePaths {
    executable_dir: PathBuf,
    resource_dir: PathBuf,
    frameworks_dir: Option<PathBuf>,
}

fn expected_target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-gnu"),
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        _ => None,
    }
}

fn open_regular_file(path: &Path) -> Result<File, ProbeError> {
    let unreadable = |error: io::Error| match error.kind() {
        io::ErrorKind::NotFound => ProbeError::Missing(format!("{} is missing", path.display())),
        io::ErrorKind::PermissionDenied => {
            ProbeError::Unreadable(format!("{} cannot be read", path.display()))
        }
        _ => ProbeError::Unreadable(format!("{} could not be read: {error}", path.display())),
    };
    let mut options = File::options();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Inspect the opened descriptor without waiting for a FIFO writer.
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path).map_err(unreadable)?;
    if !file.metadata().map_err(unreadable)?.is_file() {
        return Err(ProbeError::Malformed(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    Ok(file)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, ProbeError> {
    let file = open_regular_file(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            ProbeError::Unreadable(format!("{} could not be read: {error}", path.display()))
        })?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(ProbeError::Malformed(format!(
            "{} exceeds the manifest size limit",
            path.display()
        )));
    }
    serde_json::from_slice(&bytes).map_err(|_| {
        ProbeError::Malformed(format!("{} is not a valid Silo manifest", path.display()))
    })
}

fn readable_file(path: &Path) -> Result<(), ProbeError> {
    open_regular_file(path).map(|_| ())
}

#[cfg(any(target_os = "linux", test))]
fn sha256_file(path: &Path, deadline: Instant) -> Result<String, ProbeError> {
    let mut file = open_regular_file(path)?;
    let length = file
        .metadata()
        .map_err(|error| {
            ProbeError::Unreadable(format!(
                "{} could not be inspected: {error}",
                path.display()
            ))
        })?
        .len();
    if length > MAX_HASH_BYTES {
        return Err(ProbeError::Malformed(format!(
            "{} exceeds the packaged file-size limit",
            path.display()
        )));
    }
    let deadline = deadline.min(Instant::now() + PROCESS_TIMEOUT);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        if Instant::now() >= deadline {
            return Err(ProbeError::Timeout);
        }
        let count = file.read(&mut buffer).map_err(|error| {
            ProbeError::Unreadable(format!("{} could not be read: {error}", path.display()))
        })?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn expected_runtime_assets(
    target: &str,
) -> Option<(
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
)> {
    let approved = RUNTIME_INPUTS.targets.get(target)?;
    Some((
        "msb",
        &approved.executable_asset,
        &approved.executable_sha256,
        &approved.library_name,
        &approved.library_asset,
        &approved.library_sha256,
    ))
}

fn expected_agentd_asset(target: &str) -> Option<(&'static str, &'static str)> {
    let approved = RUNTIME_INPUTS.targets.get(target)?;
    Some((&approved.agentd_asset, &approved.agentd_sha256))
}

fn expected_git_archive(
    target: &str,
) -> Option<(&'static str, &'static str, &'static str, &'static str)> {
    match target {
        "aarch64-apple-darwin" => Some((
            "dugite-native-v2.53.0-4098283-macOS-arm64.tar.gz",
            "f9dc64635a5b62fbd7ad95db73268bbb8912255ac516d65d37bf7af22fcb8ffe",
            "git version 2.53.0",
            "macOS 12.0 (Silo declares macOS 14.0)",
        )),
        "aarch64-unknown-linux-gnu" => Some((
            "dugite-native-v2.53.0-4098283-ubuntu-arm64.tar.gz",
            "a161f45af4626bb7e0c688854bd4a9aee47cc514bca404cff0a5e3536ef1c0af",
            "git version 2.53.0.dirty",
            "glibc 2.34",
        )),
        "x86_64-unknown-linux-gnu" => Some((
            "dugite-native-v2.53.0-4098283-ubuntu-x64.tar.gz",
            "cca76aa31ad9e835e771ee7f55b73934777fbd8d16757a10d307ba06de860901",
            "git version 2.53.0",
            "glibc 2.34",
        )),
        _ => None,
    }
}

fn expected_git_executable_hashes(target: &str) -> Option<[&'static str; 4]> {
    match target {
        "aarch64-apple-darwin" => Some([
            "8b0c175c430d35d8a790ca907a37ca8742966a15f0da0b75e6747e640e24460a",
            "48bb6497160105ef852044da75147acee19502efd5f164b9a4429c3a60be7d4a",
            "a0119fd412990a160b9dd9c6866449970836801a78eef1e9dbc823a32a054690",
            "a0119fd412990a160b9dd9c6866449970836801a78eef1e9dbc823a32a054690",
        ]),
        "aarch64-unknown-linux-gnu" => Some([
            "1639d4cbfd7d27c49cbc414e7ec297919c52aade7cb91a64d4b8408f66dbb53c",
            "83a0e15428950cf52d8806223f05cc61788f965ed90fb634226a5610c852bba3",
            "3c453f3cef77616602ce893cfef9c3cefe2d8eb1860c7cd131f130f90778042e",
            "3c453f3cef77616602ce893cfef9c3cefe2d8eb1860c7cd131f130f90778042e",
        ]),
        "x86_64-unknown-linux-gnu" => Some([
            "74ea93260192ffae64e47c70c9fc54c8ca3609fcc746dc6518e1d92a54951fd4",
            "6b92b05c4588b4a5373b2b4102dbb302757d8ec6671da67cf9e4f9ccb01cd349",
            "c6ae57d2bee04dcaf52616b6915674ea51bee539a7ab22609ece4e70773d20a8",
            "c6ae57d2bee04dcaf52616b6915674ea51bee539a7ab22609ece4e70773d20a8",
        ]),
        _ => None,
    }
}

fn run_bounded_with_timeout(
    path: &Path,
    arguments: &[&str],
    environment: &[(&str, &Path)],
    timeout: Duration,
) -> Result<String, ProbeError> {
    let deadline = Instant::now() + timeout;
    readable_file(path)?;
    let mut stdout_file = tempfile::tempfile().map_err(|error| {
        ProbeError::Unavailable(format!("Could not isolate version output: {error}"))
    })?;
    let mut stderr_file = tempfile::tempfile().map_err(|error| {
        ProbeError::Unavailable(format!("Could not isolate version errors: {error}"))
    })?;
    let mut command = Command::new(path);
    command
        .args(arguments)
        .env_clear()
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_file.try_clone().map_err(|error| {
            ProbeError::Unavailable(format!("Could not capture version output: {error}"))
        })?))
        .stderr(Stdio::from(stderr_file.try_clone().map_err(|error| {
            ProbeError::Unavailable(format!("Could not capture version errors: {error}"))
        })?));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
        // The child writes to regular temporary files; enforce the output bound
        // while it runs, using only resource-limit syscalls between fork and exec.
        unsafe {
            command.pre_exec(|| {
                let zero = libc::rlimit {
                    rlim_cur: 0,
                    rlim_max: 0,
                };
                if libc::setrlimit(libc::RLIMIT_CORE, &zero) != 0 {
                    return Err(io::Error::last_os_error());
                }
                let mut limit = zero;
                if libc::getrlimit(libc::RLIMIT_FSIZE, &mut limit) != 0 {
                    return Err(io::Error::last_os_error());
                }
                let bound = (MAX_OUTPUT + 1) as libc::rlim_t;
                limit.rlim_cur = limit.rlim_cur.min(bound);
                limit.rlim_max = limit.rlim_max.min(bound);
                if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) != 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    for (name, value) in environment {
        command.env(name, value);
    }
    let mut child = command.spawn().map_err(|error| match error.kind() {
        io::ErrorKind::PermissionDenied => {
            ProbeError::Unreadable(format!("{} is not executable", path.display()))
        }
        _ => ProbeError::Unavailable(format!("{} could not run: {error}", path.display())),
    })?;
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| ProbeError::Unavailable(format!("version check failed: {error}")))?
        {
            break status;
        }
        if Instant::now() >= deadline {
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            #[cfg(not(unix))]
            let _ = child.kill();
            let _ = child.wait();
            return Err(ProbeError::Timeout);
        }
        thread::sleep(Duration::from_millis(20));
    };
    stdout_file.seek(SeekFrom::Start(0)).map_err(|error| {
        ProbeError::Unreadable(format!("version output could not be read: {error}"))
    })?;
    stderr_file.seek(SeekFrom::Start(0)).map_err(|error| {
        ProbeError::Unreadable(format!("version error output could not be read: {error}"))
    })?;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    stdout_file
        .take(MAX_OUTPUT + 1)
        .read_to_end(&mut stdout)
        .map_err(|error| {
            ProbeError::Unreadable(format!("version output could not be read: {error}"))
        })?;
    stderr_file
        .take(MAX_OUTPUT + 1)
        .read_to_end(&mut stderr)
        .map_err(|error| {
            ProbeError::Unreadable(format!("version error output could not be read: {error}"))
        })?;
    if stdout.len() > MAX_OUTPUT as usize || stderr.len() > MAX_OUTPUT as usize {
        return Err(ProbeError::Malformed(
            "version output exceeded 8 KiB".into(),
        ));
    }
    if !status.success() {
        let detail = String::from_utf8_lossy(&stderr).trim().to_owned();
        return Err(ProbeError::Unsupported(if detail.is_empty() {
            format!("version check exited with {status}")
        } else {
            detail
        }));
    }
    String::from_utf8(stdout)
        .map(|value| value.trim().to_owned())
        .map_err(|_| ProbeError::Malformed("version output was not UTF-8".into()))
}

fn run_bounded_until(
    path: &Path,
    arguments: &[&str],
    environment: &[(&str, &Path)],
    deadline: Instant,
) -> Result<String, ProbeError> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or(ProbeError::Timeout)?;
    run_bounded_with_timeout(path, arguments, environment, remaining.min(PROCESS_TIMEOUT))
}

// Local extension keeps the user-facing mapping exhaustive without leaking paths into group captions.
impl ProbeError {
    fn to_check(self, id: &str, title: &str, reinstall: bool) -> DependencyCheck {
        let recovery = if reinstall {
            REINSTALL_GUIDANCE
        } else {
            RETRY_GUIDANCE
        };
        match self {
            Self::Timeout => DependencyCheck::failure(
                id,
                title,
                CheckStatus::Timeout,
                "The check did not finish in time. No successful result was recorded.",
                RETRY_GUIDANCE,
            ),
            Self::Missing(detail) => {
                DependencyCheck::failure(id, title, CheckStatus::Unavailable, detail, recovery)
            }
            Self::Unreadable(detail) => DependencyCheck::failure(
                id,
                title,
                CheckStatus::Unavailable,
                detail,
                if reinstall {
                    "Check that your user can read and run Silo’s app files, then retry checks."
                } else {
                    RETRY_GUIDANCE
                },
            ),
            Self::Malformed(detail) => {
                DependencyCheck::failure(id, title, CheckStatus::Failed, detail, recovery)
            }
            Self::Unsupported(detail) => {
                DependencyCheck::failure(id, title, CheckStatus::Failed, detail, recovery)
            }
            Self::Unavailable(detail) => DependencyCheck::failure(
                id,
                title,
                CheckStatus::Unavailable,
                detail,
                RETRY_GUIDANCE,
            ),
        }
    }
}

fn system_check(deadline: Instant) -> DependencyCheck {
    let id = "system-os";
    let title = "Supported OS";
    if Instant::now() >= deadline {
        return ProbeError::Timeout.to_check(id, title, false);
    }
    if expected_target().is_none() {
        return DependencyCheck::failure(
            id,
            title,
            CheckStatus::Failed,
            format!(
                "{} {} is not supported by this Silo build.",
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
            "Use Silo on macOS 14 or later on Apple silicon, or a supported Linux build.",
        );
    }
    #[cfg(target_os = "macos")]
    {
        let output = run_bounded_until(
            Path::new("/usr/bin/sw_vers"),
            &["-productVersion"],
            &[],
            deadline,
        );
        let version = match output {
            Ok(value) => value,
            Err(error) => return error.to_check(id, title, false),
        };
        let major = version
            .split('.')
            .next()
            .and_then(|value| value.parse::<u32>().ok());
        if major.is_none() {
            return ProbeError::Malformed("macOS returned an invalid version.".into())
                .to_check(id, title, false);
        }
        if major.unwrap() < 14 {
            return DependencyCheck::failure(
                id,
                title,
                CheckStatus::Failed,
                format!("macOS {version} is not supported by this Silo build."),
                "Update macOS to version 14 or later, then retry checks.",
            );
        }
        return DependencyCheck::pass(id, title, format!("macOS {version} · Apple silicon"));
    }
    #[cfg(target_os = "linux")]
    {
        let target = expected_target().expect("supported Linux target");
        let version = glibc_version();
        return linux_system_version_result(&version, target);
    }
}

#[cfg(target_os = "linux")]
fn glibc_version() -> String {
    use std::ffi::CStr;
    unsafe extern "C" {
        fn gnu_get_libc_version() -> *const libc::c_char;
    }
    // SAFETY: glibc owns a process-lifetime NUL-terminated version string.
    unsafe {
        CStr::from_ptr(gnu_get_libc_version())
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(any(target_os = "linux", test))]
fn parse_major_minor(value: &str) -> Option<(u32, u32)> {
    let mut parts = value.split('.');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

#[cfg(any(target_os = "linux", test))]
fn linux_system_version_result(version: &str, target: &str) -> DependencyCheck {
    let Some((major, minor)) = parse_major_minor(version) else {
        return ProbeError::Malformed("glibc returned an invalid version.".into()).to_check(
            "system-os",
            "Supported OS",
            false,
        );
    };
    if (major, minor) < (2, 34) {
        return DependencyCheck::failure(
            "system-os",
            "Supported OS",
            CheckStatus::Failed,
            format!("glibc {version} is older than the required 2.34 for {target}."),
            "Upgrade to a Linux distribution with glibc 2.34 or later, then retry checks.",
        );
    }
    let architecture = if target.starts_with("aarch64") {
        "arm64"
    } else {
        "x86_64"
    };
    DependencyCheck::pass(
        "system-os",
        "Supported OS",
        format!("Linux {architecture} · glibc {version}"),
    )
}

fn virtualization_check(deadline: Instant) -> DependencyCheck {
    let id = "system-virtualization";
    let title = "Virtualization";
    if Instant::now() >= deadline {
        return ProbeError::Timeout.to_check(id, title, false);
    }
    #[cfg(target_os = "macos")]
    {
        return match run_bounded_until(
            Path::new("/usr/sbin/sysctl"),
            &["-n", "kern.hv_support"],
            &[],
            deadline,
        ) {
            Ok(value) if value == "1" => {
                DependencyCheck::pass(id, title, "Apple Hypervisor available")
            }
            Ok(_) => DependencyCheck::failure(id, title, CheckStatus::Failed,
                "Apple Hypervisor support is unavailable on this Mac.",
                "Use an Apple silicon Mac with macOS 14 or later. If Silo is inside a VM, its host must support and enable nested virtualization."),
            Err(error) => error.to_check(id, title, false),
        };
    }
    #[cfg(target_os = "linux")]
    {
        let device = match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/kvm")
        {
            Ok(file) => file,
            Err(error) => return kvm_open_failure(error),
        };
        use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
        // KVM_GET_API_VERSION is _IO(KVMIO, 0x00). It queries only and never creates a VM.
        let version = unsafe { libc::ioctl(device.as_raw_fd(), 0xAE00) };
        return if version == 12 {
            // KVM_CREATE_VM is _IO(KVMIO, 0x01) with the default machine type. It
            // fails when firmware disabled virtualization or another hypervisor
            // (VirtualBox, VMware) holds it. The empty VM is closed immediately.
            let vm = unsafe { libc::ioctl(device.as_raw_fd(), 0xAE01, 0) };
            kvm_create_vm_result(if vm < 0 {
                Err(io::Error::last_os_error())
            } else {
                // SAFETY: KVM_CREATE_VM returned a new descriptor this process owns.
                drop(unsafe { OwnedFd::from_raw_fd(vm) });
                Ok(())
            })
        } else if version < 0 {
            kvm_api_query_failure(io::Error::last_os_error())
        } else {
            DependencyCheck::failure(
                id,
                title,
                CheckStatus::Failed,
                format!("KVM API {version} is incompatible; Silo requires API 12."),
                "Update or replace the host KVM implementation, then retry checks.",
            )
        };
    }
}

#[cfg(any(target_os = "linux", test))]
fn kvm_api_query_failure(error: io::Error) -> DependencyCheck {
    if kvm_hardware_disabled(&error) {
        return DependencyCheck::failure(
            "system-virtualization",
            "Virtualization",
            CheckStatus::Failed,
            "KVM is installed, but hardware virtualization is unavailable.",
            KVM_FIRMWARE_GUIDANCE,
        );
    }
    DependencyCheck::failure(
        "system-virtualization",
        "Virtualization",
        CheckStatus::Unavailable,
        format!("KVM API query failed: {error}"),
        "Check KVM access on this device, then retry checks.",
    )
}

#[cfg(any(target_os = "linux", test))]
const KVM_FIRMWARE_GUIDANCE: &str = "Turn on hardware virtualization (Intel VT-x or AMD-V/SVM) in your device’s firmware (BIOS/UEFI) settings, then restart. Inside a VM, enable nested virtualization on its host. Then retry checks.";

/// ENODEV/ENXIO: the KVM module is loaded but the CPU's virtualization support
/// is disabled in firmware or unavailable.
#[cfg(any(target_os = "linux", test))]
fn kvm_hardware_disabled(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(libc::ENODEV | libc::ENXIO))
}

#[cfg(any(target_os = "linux", test))]
fn kvm_create_vm_result(result: io::Result<()>) -> DependencyCheck {
    let (id, title) = ("system-virtualization", "Virtualization");
    match result {
        Ok(()) => DependencyCheck::pass(id, title, "KVM API 12 available"),
        Err(error) if error.raw_os_error() == Some(libc::EBUSY) => DependencyCheck::failure(
            id,
            title,
            CheckStatus::Failed,
            "Another hypervisor is using hardware virtualization, so KVM cannot create VMs.",
            "Quit other virtualization software such as VirtualBox or VMware, then retry checks.",
        ),
        Err(error) if kvm_hardware_disabled(&error) => DependencyCheck::failure(
            id,
            title,
            CheckStatus::Failed,
            "KVM is installed, but hardware virtualization is unavailable.",
            KVM_FIRMWARE_GUIDANCE,
        ),
        Err(error) => DependencyCheck::failure(
            id,
            title,
            CheckStatus::Unavailable,
            format!("KVM could not create a test VM: {error}"),
            "Retry checks. If this keeps happening, check that KVM works on this device, for example with kvm-ok.",
        ),
    }
}

#[cfg(any(target_os = "linux", test))]
fn kvm_open_failure(error: io::Error) -> DependencyCheck {
    let (status, detail, recovery) = match error.kind() {
        _ if kvm_hardware_disabled(&error) => (CheckStatus::Failed,
            "/dev/kvm exists, but hardware virtualization is unavailable.".to_owned(),
            KVM_FIRMWARE_GUIDANCE),
        io::ErrorKind::NotFound => (CheckStatus::Unavailable,
            "/dev/kvm is unavailable on this device.".to_owned(),
            "Enable hardware virtualization in your device settings and enable KVM using your Linux distribution’s instructions. Inside a VM, enable nested virtualization on its host. Then retry checks."),
        io::ErrorKind::PermissionDenied => (CheckStatus::Failed,
            "Silo cannot open /dev/kvm for this user.".to_owned(),
            "Ask your administrator to grant your user read and write access to /dev/kvm, usually through the kvm group. Sign out and back in, then retry checks."),
        _ => (CheckStatus::Unavailable, format!("Silo could not open /dev/kvm: {error}"),
            "Retry checks. If this keeps happening, check that KVM is enabled and /dev/kvm is accessible on this device."),
    };
    DependencyCheck::failure(
        "system-virtualization",
        "Virtualization",
        status,
        detail,
        recovery,
    )
}

#[cfg(target_os = "macos")]
fn verify_macos_signature(path: &Path, deadline: Instant) -> Result<(), ProbeError> {
    run_bounded_until(
        Path::new("/usr/bin/codesign"),
        &["--verify", "--strict", path.to_string_lossy().as_ref()],
        &[],
        deadline,
    )
    .map(|_| ())
}

#[cfg(target_os = "linux")]
fn verify_linux_hash(path: &Path, expected: &str, deadline: Instant) -> Result<(), ProbeError> {
    let actual = sha256_file(path, deadline)?;
    if actual == expected {
        Ok(())
    } else {
        Err(ProbeError::Malformed(format!(
            "{} failed its packaged checksum.",
            path.file_name().unwrap_or_default().to_string_lossy()
        )))
    }
}

fn microsandbox_check(paths: &ProbePaths, deadline: Instant) -> DependencyCheck {
    let id = "runtime-microsandbox";
    let title = "MicroSandbox runtime";
    let manifest_path = paths.resource_dir.join("microsandbox/manifest.json");
    let manifest: MicrosandboxManifest = match read_json(&manifest_path) {
        Ok(value) => value,
        Err(error) => return error.to_check(id, title, true),
    };
    if !runtime_manifest_matches(&manifest) {
        return ProbeError::Malformed(
            "The bundled MicroSandbox manifest does not match this Silo build.".into(),
        )
        .to_check(id, title, true);
    }
    let executable = paths.executable_dir.join(&manifest.executable.bundled_name);
    #[cfg(target_os = "macos")]
    let library = paths
        .frameworks_dir
        .as_ref()
        .expect("macOS frameworks path")
        .join(&manifest.library.bundled_name);
    #[cfg(target_os = "linux")]
    let library = crate::runtime::bundled_runtime_library(
        &executable,
        &paths.resource_dir,
        tauri::utils::platform::bundle_type(),
    );
    for candidate in [&executable, &library] {
        if let Err(error) = readable_file(candidate) {
            return error.to_check(id, title, true);
        }
    }
    #[cfg(target_os = "macos")]
    for candidate in [&executable, &library] {
        if let Err(error) = verify_macos_signature(candidate, deadline) {
            return error.to_check(id, title, true);
        }
    }
    #[cfg(target_os = "linux")]
    for (candidate, expected) in [
        (&executable, manifest.executable.sha256.as_str()),
        (&library, manifest.library.sha256.as_str()),
    ] {
        if let Err(error) = verify_linux_hash(candidate, expected, deadline) {
            return error.to_check(id, title, true);
        }
    }
    let home = match tempfile::tempdir() {
        Ok(value) => value,
        Err(error) => {
            return ProbeError::Unavailable(format!("Could not isolate the version check: {error}"))
                .to_check(id, title, true)
        }
    };
    let output = run_bounded_until(
        &executable,
        &["--version"],
        &[
            ("HOME", home.path()),
            ("MSB_HOME", home.path()),
            ("MSB_PATH", &executable),
            ("MSB_LIBKRUNFW_PATH", &library),
        ],
        deadline,
    );
    microsandbox_version_result(output)
}

fn runtime_manifest_matches(manifest: &MicrosandboxManifest) -> bool {
    manifest.schema_version == 2
        && manifest.microsandbox_version == RUNTIME_INPUTS.microsandbox_version
        && manifest.libkrunfw_version == RUNTIME_INPUTS.libkrunfw_version
        && Some(manifest.target_triple.as_str()) == expected_target()
        && expected_runtime_assets(&manifest.target_triple).is_some_and(
            |(
                executable_name,
                executable_asset,
                executable_sha,
                library_name,
                library_asset,
                library_sha,
            )| {
                manifest.executable.bundled_name == executable_name
                    && valid_sha256(&manifest.executable.sha256)
                    && manifest.executable.source_commit == RUNTIME_INPUTS.source_commit
                    && manifest.executable.source_archive_sha256
                        == RUNTIME_INPUTS.source_archive_sha256
                    && manifest.executable.patch_sha256s
                        == RUNTIME_INPUTS
                            .patches
                            .iter()
                            .map(|patch| patch.sha256.clone())
                            .collect::<Vec<_>>()
                    && manifest.executable.toolchain == RUNTIME_INPUTS.toolchain
                    && manifest.executable.features == RUNTIME_INPUTS.features
                    && manifest.executable.official_release_asset == executable_asset
                    && manifest.executable.official_release_sha256 == executable_sha
                    && expected_agentd_asset(&manifest.target_triple).is_some_and(|(asset, sha)| {
                        manifest.executable.embedded_agentd_release_asset == asset
                            && manifest.executable.embedded_agentd_release_sha256 == sha
                    })
                    && manifest.library.bundled_name == library_name
                    && manifest.library.release_asset == library_asset
                    && manifest.library.sha256 == library_sha
            },
        )
}

fn microsandbox_version_result(output: Result<String, ProbeError>) -> DependencyCheck {
    match output {
        Ok(value) if value == format!("msb {}", RUNTIME_INPUTS.microsandbox_version) => {
            DependencyCheck::pass(
                "runtime-microsandbox",
                "MicroSandbox runtime",
                format!(
                    "Bundled msb {} · libkrunfw {}",
                    RUNTIME_INPUTS.microsandbox_version, RUNTIME_INPUTS.libkrunfw_version
                ),
            )
        }
        Ok(value) => ProbeError::Malformed(format!(
            "Bundled msb returned an unexpected version: {value}"
        ))
        .to_check("runtime-microsandbox", "MicroSandbox runtime", true),
        Err(error) => error.to_check("runtime-microsandbox", "MicroSandbox runtime", true),
    }
}

fn git_checks(paths: &ProbePaths, deadline: Instant) -> [DependencyCheck; 2] {
    if Instant::now() >= deadline {
        return [
            ProbeError::Timeout.to_check("tool-git", "Git", true),
            ProbeError::Timeout.to_check("tool-git-lfs", "Git LFS", true),
        ];
    }
    let manifest_path = paths.resource_dir.join("git-support/manifest.json");
    let manifest: GitManifest = match read_json(&manifest_path) {
        Ok(value) => value,
        Err(error) => {
            return [
                error.to_check("tool-git", "Git", true),
                DependencyCheck::failure(
                    "tool-git-lfs",
                    "Git LFS",
                    CheckStatus::Unavailable,
                    "Git LFS was not checked because the bundled Git manifest is unavailable.",
                    "Resolve the Git check above, then retry checks.",
                ),
            ]
        }
    };
    let expected_archive = expected_git_archive(&manifest.target_triple);
    let expected_hashes = expected_git_executable_hashes(&manifest.target_triple);
    let hashes = &manifest.executable_sha256;
    let paths_are_fixed = git_paths_are_fixed(&manifest);
    let executable_hashes = [
        hashes.git.as_str(),
        hashes.git_lfs.as_str(),
        hashes.git_remote_http.as_str(),
        hashes.git_remote_https.as_str(),
    ];
    let executable_hashes_match = executable_hashes.into_iter().all(valid_sha256)
        && expected_hashes.is_some_and(|expected| executable_hashes == expected);
    if manifest.schema_version != 1
        || manifest.distribution != "desktop/dugite-native"
        || manifest.distribution_release != "v2.53.0-4"
        || manifest.distribution_commit != "4098283a7ecb8a227b9d43580336c78a06f90e5d"
        || manifest.git_version != EXPECTED_GIT
        || manifest.git_lfs_version != EXPECTED_GIT_LFS
        || Some(manifest.target_triple.as_str()) != expected_target()
        || !paths_are_fixed
        || !executable_hashes_match
        || expected_archive.is_none()
        || expected_archive.is_some_and(|(name, sha, version, platform)| {
            manifest.archive.name != name
                || manifest.archive.sha256 != sha
                || manifest.git_version_output != version
                || manifest.minimum_platform != platform
        })
    {
        let error = ProbeError::Malformed(
            "The bundled Git manifest does not match this Silo build.".into(),
        );
        return [
            error.to_check("tool-git", "Git", true),
            DependencyCheck::failure(
                "tool-git-lfs",
                "Git LFS",
                CheckStatus::Unavailable,
                "Git LFS was not checked because the bundled Git manifest is invalid.",
                "Resolve the Git check above, then retry checks.",
            ),
        ];
    }
    let executables = &manifest.paths.packaged_executables;
    let git = paths.executable_dir.join(&executables.git);
    let lfs = paths.executable_dir.join(&executables.git_lfs);
    let http = paths.executable_dir.join(&executables.git_remote_http);
    let https = paths.executable_dir.join(&executables.git_remote_https);
    #[cfg(target_os = "macos")]
    for candidate in [&git, &lfs, &http, &https] {
        if let Err(error) = verify_macos_signature(candidate, deadline) {
            return [
                error.to_check("tool-git", "Git", true),
                DependencyCheck::failure(
                    "tool-git-lfs",
                    "Git LFS",
                    CheckStatus::Unavailable,
                    "Git LFS was not checked because bundled Git integrity could not be verified.",
                    "Resolve the Git check above, then retry checks.",
                ),
            ];
        }
    }
    #[cfg(target_os = "linux")]
    for (candidate, expected) in [
        (&git, manifest.executable_sha256.git.as_str()),
        (&lfs, manifest.executable_sha256.git_lfs.as_str()),
        (&http, manifest.executable_sha256.git_remote_http.as_str()),
        (&https, manifest.executable_sha256.git_remote_https.as_str()),
    ] {
        if let Err(error) = verify_linux_hash(candidate, expected, deadline) {
            return [
                error.to_check("tool-git", "Git", true),
                DependencyCheck::failure(
                    "tool-git-lfs",
                    "Git LFS",
                    CheckStatus::Unavailable,
                    "Git LFS was not checked because bundled Git integrity could not be verified.",
                    "Resolve the Git check above, then retry checks.",
                ),
            ];
        }
    }
    let home = match tempfile::tempdir() {
        Ok(value) => value,
        Err(error) => {
            return [
                ProbeError::Unavailable(format!("Could not isolate the Git checks: {error}"))
                    .to_check("tool-git", "Git", true),
                DependencyCheck::failure(
                    "tool-git-lfs",
                    "Git LFS",
                    CheckStatus::Unavailable,
                    "Git LFS was not checked.",
                    "Retry checks.",
                ),
            ]
        }
    };
    let exec_path = paths.executable_dir.as_path();
    let template_path = paths
        .resource_dir
        .join("git-support")
        .join(&manifest.paths.templates);
    let null = Path::new("/dev/null");
    let environment = [
        ("HOME", home.path()),
        ("XDG_CONFIG_HOME", home.path()),
        ("GIT_CONFIG_NOSYSTEM", Path::new("1")),
        ("GIT_CONFIG_SYSTEM", null),
        ("GIT_CONFIG_GLOBAL", null),
        ("GIT_EXEC_PATH", exec_path),
        ("GIT_TEMPLATE_DIR", template_path.as_path()),
        ("PATH", exec_path),
    ];
    let git_result = git_version_result(
        run_bounded_until(&git, &["--version"], &environment, deadline),
        &manifest.git_version_output,
    );
    let lfs_result = git_lfs_version_result(run_bounded_until(
        &lfs,
        &["version"],
        &environment,
        deadline,
    ));
    [git_result, lfs_result]
}

fn git_paths_are_fixed(manifest: &GitManifest) -> bool {
    manifest.paths.git == "bin/git"
        && manifest.paths.git_exec_path == "libexec/git-core"
        && manifest.paths.git_lfs == "libexec/git-core/git-lfs"
        && manifest.paths.templates == "share/git-core/templates"
        && manifest.paths.packaged_resource_directory == "git-support"
        && manifest.paths.packaged_executables.directory == "executableSibling"
        && manifest.paths.packaged_executables.git == "git"
        && manifest.paths.packaged_executables.git_lfs == "git-lfs"
        && manifest.paths.packaged_executables.git_remote_http == "git-remote-http"
        && manifest.paths.packaged_executables.git_remote_https == "git-remote-https"
        && ((manifest.target_triple.contains("linux")
            && manifest.paths.certificate_bundle.as_deref() == Some("ssl/cacert.pem"))
            || (manifest.target_triple.contains("apple")
                && manifest.paths.certificate_bundle.is_none()))
}

fn git_version_result(
    output: Result<String, ProbeError>,
    expected_output: &str,
) -> DependencyCheck {
    match output {
        Ok(value) if value == expected_output => {
            DependencyCheck::pass("tool-git", "Git", format!("Bundled Git {EXPECTED_GIT}"))
        }
        Ok(value) => ProbeError::Malformed(format!(
            "Bundled Git returned an unexpected version: {value}"
        ))
        .to_check("tool-git", "Git", true),
        Err(error) => error.to_check("tool-git", "Git", true),
    }
}

fn git_lfs_version_result(output: Result<String, ProbeError>) -> DependencyCheck {
    match output {
        Ok(value)
            if value.starts_with(&format!("git-lfs/{EXPECTED_GIT_LFS} "))
                || value == format!("git-lfs/{EXPECTED_GIT_LFS}") =>
        {
            DependencyCheck::pass(
                "tool-git-lfs",
                "Git LFS",
                format!("Bundled Git LFS {EXPECTED_GIT_LFS}"),
            )
        }
        Ok(value) => ProbeError::Malformed(format!(
            "Bundled Git LFS returned an unexpected version: {value}"
        ))
        .to_check("tool-git-lfs", "Git LFS", true),
        Err(error) => error.to_check("tool-git-lfs", "Git LFS", true),
    }
}

struct CheckGroup<'a> {
    budget: Duration,
    run: Box<dyn Fn(Instant) -> Vec<DependencyCheck> + 'a>,
}

fn timed_out(checks: &[DependencyCheck]) -> bool {
    checks
        .iter()
        .any(|check| matches!(check.status, CheckStatus::Timeout))
}

/// Runs each group under its own deadline. A group that timed out (usually a
/// busy device) runs once more with a fresh budget before it is reported.
fn run_groups(request_id: String, groups: &[CheckGroup]) -> DependencyReport {
    let run = |group: &CheckGroup| (group.run)(Instant::now() + group.budget);
    let checks = groups
        .iter()
        .flat_map(|group| {
            let first = run(group);
            if timed_out(&first) {
                run(group)
            } else {
                first
            }
        })
        .collect();
    DependencyReport {
        schema_version: 1,
        request_id,
        checked_at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        checks,
    }
}

fn collect(request_id: String, paths: ProbePaths) -> DependencyReport {
    run_groups(
        request_id,
        &[
            CheckGroup {
                budget: SYSTEM_BUDGET,
                run: Box::new(|deadline| {
                    vec![system_check(deadline), virtualization_check(deadline)]
                }),
            },
            CheckGroup {
                budget: MICROSANDBOX_BUDGET,
                run: Box::new(|deadline| vec![microsandbox_check(&paths, deadline)]),
            },
            CheckGroup {
                budget: GIT_BUDGET,
                run: Box::new(|deadline| git_checks(&paths, deadline).into()),
            },
        ],
    )
}

#[tauri::command]
pub async fn read_dependencies(
    app: AppHandle,
    request_id: String,
) -> Result<DependencyReport, String> {
    eprintln!("dependency check request: {request_id}");
    if request_id.trim().is_empty() || request_id.len() > 128 {
        return Err("Invalid dependency-check request ID".into());
    }
    let executable_dir = crate::bundled_tools::directory(&app)?;
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|error| format!("Silo resource directory is unavailable: {error}"))?;
    #[cfg(target_os = "macos")]
    let frameworks_dir = executable_dir
        .parent()
        .map(|contents| contents.join("Frameworks"));
    #[cfg(not(target_os = "macos"))]
    let frameworks_dir = None;
    let report = tauri::async_runtime::spawn_blocking(move || {
        collect(
            request_id,
            ProbePaths {
                executable_dir,
                resource_dir,
                frameworks_dir,
            },
        )
    })
    .await
    .map_err(|error| format!("Dependency checks could not run: {error}"))?;
    eprintln!("dependency check response: {}", report.checks.len());
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version_probe(script: &str, arguments: &[&str]) -> Result<String, ProbeError> {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("version-probe");
        crate::test_support::write_shell_script(&executable, script);
        run_bounded_with_timeout(
            &executable,
            arguments,
            &[("HOME", directory.path())],
            Duration::from_secs(3),
        )
    }

    #[test]
    fn version_probe_passes_literal_arguments_and_uses_an_isolated_home_and_locale() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("version-probe");
        crate::test_support::write_shell_script(
            &executable,
            "if IFS= read -r line; then exit 1; fi\nprintf ' \\t%s\\n%s\\n%s\\n%s \\n' \"$1\" \"$HOME\" \"$LANG\" \"$LC_ALL\"\nprintf warning >&2",
        );
        let value = "literal '$value'; $(false)";
        assert_eq!(
            run_bounded_with_timeout(
                &executable,
                &[value],
                &[("HOME", directory.path())],
                Duration::from_secs(3),
            )
            .unwrap(),
            format!("{value}\n{}\nC\nC", directory.path().display())
        );
    }

    #[test]
    fn version_probe_accepts_the_output_limit_and_rejects_overflow_on_either_stream() {
        let script = "if [ \"$2\" = stderr ]; then exec 1>&2; fi\ni=0\nwhile [ \"$i\" -lt \"$1\" ]; do printf x; i=$((i+1)); done";
        for stream in ["stdout", "stderr"] {
            let at_limit = version_probe(script, &["8192", stream]).unwrap();
            assert_eq!(
                at_limit,
                if stream == "stdout" {
                    "x".repeat(8192)
                } else {
                    String::new()
                }
            );
            match version_probe(script, &["8193", stream]) {
                Err(ProbeError::Malformed(detail)) => {
                    assert_eq!(detail, "version output exceeded 8 KiB")
                }
                result => panic!("oversized {stream} was not rejected: {result:?}"),
            }
        }
    }

    #[test]
    fn version_probe_rejects_successful_non_utf8_output() {
        match version_probe("printf '\\377'", &[]) {
            Err(ProbeError::Malformed(detail)) => {
                assert_eq!(detail, "version output was not UTF-8")
            }
            result => panic!("invalid UTF-8 was not rejected: {result:?}"),
        }
    }

    #[test]
    fn failed_version_probe_reports_stderr_or_exit_status_instead_of_successful_stdout() {
        match version_probe(
            "printf 'msb 0.7.6'; printf ' specific failure \\n' >&2; exit 23",
            &[],
        ) {
            Err(ProbeError::Unsupported(detail)) => assert_eq!(detail, "specific failure"),
            result => panic!("failed probe was not rejected: {result:?}"),
        }
        match version_probe("printf 'msb 0.7.6'; exit 7", &[]) {
            Err(ProbeError::Unsupported(detail)) => {
                assert_eq!(detail, "version check exited with exit status: 7")
            }
            result => panic!("failed probe without stderr was not rejected: {result:?}"),
        }
    }

    #[test]
    fn version_probe_distinguishes_missing_non_regular_and_non_executable_files() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("version-probe");
        let probe = |path: &Path| {
            run_bounded_with_timeout(
                path,
                &[],
                &[("HOME", directory.path())],
                Duration::from_secs(3),
            )
        };
        assert!(matches!(probe(&executable), Err(ProbeError::Missing(_))));
        assert!(matches!(
            probe(directory.path()),
            Err(ProbeError::Malformed(_))
        ));
        fs::write(&executable, "#!/bin/sh\nprintf should-not-run").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(matches!(probe(&executable), Err(ProbeError::Unreadable(_))));
    }

    #[test]
    fn manifest_read_distinguishes_missing_unreadable_corrupt_and_valid_files() {
        let directory = tempfile::tempdir().unwrap();
        let manifest = directory.path().join("manifest.json");
        assert!(matches!(
            read_json::<serde_json::Value>(&manifest),
            Err(ProbeError::Missing(_))
        ));
        let blocker = directory.path().join("file");
        fs::write(&blocker, "preserve this file").unwrap();
        assert!(matches!(
            read_json::<serde_json::Value>(&blocker.join("manifest.json")),
            Err(ProbeError::Unreadable(_))
        ));
        assert_eq!(fs::read_to_string(&blocker).unwrap(), "preserve this file");
        for bytes in [b"{broken".as_slice(), b"\xff", b""] {
            fs::write(&manifest, bytes).unwrap();
            assert!(matches!(
                read_json::<serde_json::Value>(&manifest),
                Err(ProbeError::Malformed(_))
            ));
            assert_eq!(fs::read(&manifest).unwrap(), bytes);
        }
        fs::write(&manifest, r#"{"schemaVersion":2}"#).unwrap();
        assert_eq!(
            read_json::<serde_json::Value>(&manifest).unwrap(),
            serde_json::json!({"schemaVersion":2})
        );
    }

    #[test]
    fn missing_runtime_manifest_fails_existing_runtime_check_without_installing() {
        let directory = tempfile::tempdir().unwrap();
        let paths = ProbePaths {
            executable_dir: directory.path().join("bin"),
            resource_dir: directory.path().join("resources"),
            frameworks_dir: Some(directory.path().join("Frameworks")),
        };
        let check = microsandbox_check(&paths, Instant::now() + MICROSANDBOX_BUDGET);
        assert_eq!(check.id, "runtime-microsandbox");
        assert_ne!(check.status, CheckStatus::Pass);
        assert_eq!(check.remediation.as_deref(), Some(REINSTALL_GUIDANCE));
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[test]
    fn checked_in_runtime_patches_match_packaged_build_pins() {
        // runtime-inputs.json is the single list of pinned patches. Every listed
        // patch must match its checked-in bytes, and every checked-in patch must
        // be listed, so adding a patch cannot silently skip this check.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut listed = std::collections::BTreeSet::new();
        for input in &RUNTIME_INPUTS.patches {
            assert!(
                input.path.starts_with("patches/") && !input.path.contains(".."),
                "{}",
                input.path
            );
            let bytes = fs::read(root.join(&input.path))
                .unwrap_or_else(|error| panic!("{}: {error}", input.path));
            assert_eq!(
                format!("{:x}", Sha256::digest(&bytes)),
                input.sha256,
                "{}",
                input.path
            );
            assert!(
                listed.insert(input.path.clone()),
                "{} is listed twice",
                input.path
            );
        }
        let checked_in: std::collections::BTreeSet<_> = fs::read_dir(root.join("patches"))
            .unwrap()
            .map(|entry| format!("patches/{}", entry.unwrap().file_name().to_string_lossy()))
            .filter(|path| path.ends_with(".patch"))
            .collect();
        assert_eq!(checked_in, listed);
        assert!(!listed.is_empty());
    }

    #[test]
    fn recovery_distinguishes_bundle_damage_from_temporary_probe_failures() {
        for error in [
            ProbeError::Missing("missing runtime".into()),
            ProbeError::Malformed("checksum mismatch".into()),
            ProbeError::Unsupported("incompatible runtime".into()),
        ] {
            let check = error.to_check("runtime-microsandbox", "Runtime", true);
            assert_ne!(check.status, CheckStatus::Pass);
            assert_eq!(check.remediation.as_deref(), Some(REINSTALL_GUIDANCE));
        }
        for packaged in [true, false] {
            for error in [
                ProbeError::Timeout,
                ProbeError::Unavailable("probe could not run".into()),
            ] {
                let check = error.to_check("check", "Check", packaged);
                assert_ne!(check.status, CheckStatus::Pass);
                assert_eq!(check.remediation.as_deref(), Some(RETRY_GUIDANCE));
            }
        }
        let unreadable =
            ProbeError::Unreadable("permission denied".into()).to_check("runtime", "Runtime", true);
        assert!(unreadable
            .remediation
            .as_deref()
            .unwrap()
            .contains("read and run"));
        assert!(!unreadable
            .remediation
            .as_deref()
            .unwrap()
            .contains("Reinstall"));
    }

    #[test]
    fn host_recovery_names_the_requirement_without_reinstalling_silo() {
        let old_linux = linux_system_version_result("2.33", "x86_64-unknown-linux-gnu");
        assert!(old_linux
            .remediation
            .unwrap()
            .contains("Linux distribution with glibc 2.34"));
        let missing = kvm_open_failure(io::Error::from(io::ErrorKind::NotFound));
        assert_eq!(missing.status, CheckStatus::Unavailable);
        assert!(missing
            .remediation
            .as_deref()
            .unwrap()
            .contains("nested virtualization"));
        let denied = kvm_open_failure(io::Error::from(io::ErrorKind::PermissionDenied));
        assert_eq!(denied.status, CheckStatus::Failed);
        assert!(denied
            .remediation
            .as_deref()
            .unwrap()
            .contains("read and write access to /dev/kvm"));
        assert!(denied
            .remediation
            .as_deref()
            .unwrap()
            .contains("Sign out and back in"));
        let transient = kvm_open_failure(io::Error::from(io::ErrorKind::Interrupted));
        assert_eq!(transient.status, CheckStatus::Unavailable);
        for check in [missing, denied, transient] {
            assert!(!check.remediation.unwrap().contains("Reinstall"));
        }
    }

    #[test]
    fn kvm_api_query_without_hardware_virtualization_points_to_firmware() {
        for errno in [libc::ENODEV, libc::ENXIO] {
            let check = kvm_api_query_failure(io::Error::from_raw_os_error(errno));
            assert_eq!(check.status, CheckStatus::Failed, "{errno}");
            assert!(check.remediation.unwrap().contains("firmware"));
        }
        let other = kvm_api_query_failure(io::Error::from_raw_os_error(libc::EIO));
        assert_eq!(other.status, CheckStatus::Unavailable);
        assert!(other.detail.contains("API query failed"));
    }

    #[test]
    fn kvm_without_hardware_virtualization_points_to_firmware_settings_not_retry() {
        for errno in [libc::ENODEV, libc::ENXIO] {
            let check = kvm_open_failure(io::Error::from_raw_os_error(errno));
            assert_eq!(check.status, CheckStatus::Failed, "{errno}");
            let remediation = check.remediation.unwrap();
            assert!(remediation.contains("firmware"), "{remediation}");
            assert!(!remediation.starts_with("Retry checks"), "{remediation}");
        }
    }

    #[test]
    fn kvm_vm_creation_failure_names_another_hypervisor_or_the_error() {
        assert!(kvm_create_vm_result(Ok(())).status == CheckStatus::Pass);
        let busy = kvm_create_vm_result(Err(io::Error::from_raw_os_error(libc::EBUSY)));
        assert_eq!(busy.status, CheckStatus::Failed);
        assert!(busy.remediation.unwrap().contains("VirtualBox"));
        let firmware = kvm_create_vm_result(Err(io::Error::from_raw_os_error(libc::ENODEV)));
        assert!(firmware.remediation.unwrap().contains("firmware"));
        let other = kvm_create_vm_result(Err(io::Error::from_raw_os_error(libc::ENOMEM)));
        assert_eq!(other.status, CheckStatus::Unavailable);
        assert!(other.detail.contains("could not create a test VM"));
    }

    #[test]
    fn maps_linux_glibc_boundaries_to_actual_check_results() {
        assert_eq!(
            linux_system_version_result("2.34", "aarch64-unknown-linux-gnu"),
            DependencyCheck::pass("system-os", "Supported OS", "Linux arm64 · glibc 2.34")
        );
        assert_eq!(
            linux_system_version_result("2.33", "x86_64-unknown-linux-gnu").status,
            CheckStatus::Failed
        );
        assert_eq!(
            linux_system_version_result("broken", "x86_64-unknown-linux-gnu").status,
            CheckStatus::Failed
        );
    }

    #[test]
    fn oversized_manifest_is_rejected_before_deserialization() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("manifest.json");
        let mut bytes = vec![b' '; MAX_MANIFEST_BYTES as usize];
        bytes.extend_from_slice(b"{}");
        fs::write(&path, bytes).unwrap();
        assert!(matches!(
            read_json::<serde_json::Value>(&path),
            Err(ProbeError::Malformed(_))
        ));
        fs::write(&path, b"{}").unwrap();
        assert_eq!(
            read_json::<serde_json::Value>(&path).unwrap(),
            serde_json::json!({})
        );
    }

    #[test]
    fn non_file_manifest_is_bundle_damage_rather_than_a_permission_error() {
        let directory = tempfile::tempdir().unwrap();
        let error = read_json::<serde_json::Value>(directory.path()).unwrap_err();
        assert!(matches!(error, ProbeError::Malformed(_)));
    }

    #[cfg(unix)]
    #[test]
    fn fifo_manifest_is_rejected_without_waiting_for_a_writer() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("manifest.json");
        let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let module = module_path!().split_once("::").unwrap().1;
        let helper = format!("{module}::fifo_manifest_reader_helper");
        let result = run_bounded_with_timeout(
            &std::env::current_exe().unwrap(),
            &["--exact", &helper],
            &[("SILO_TEST_MANIFEST_FIFO", path.as_path())],
            PROCESS_TIMEOUT,
        );
        let output = result.expect("FIFO manifest reader must finish without a writer");
        assert!(output.contains("1 passed"), "{output}");
    }

    #[test]
    fn dependency_hash_rejects_directories_and_hashes_valid_files() {
        let directory = tempfile::tempdir().unwrap();
        assert!(matches!(
            sha256_file(directory.path(), Instant::now() + PROCESS_TIMEOUT),
            Err(ProbeError::Malformed(_))
        ));
        let path = directory.path().join("binary");
        fs::write(&path, b"abc").unwrap();
        assert_eq!(
            sha256_file(&path, Instant::now() + PROCESS_TIMEOUT).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[cfg(unix)]
    #[test]
    fn dependency_hash_fifo_is_rejected_without_waiting_for_a_writer() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("git");
        let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let module = module_path!().split_once("::").unwrap().1;
        let helper = format!("{module}::dependency_hash_fifo_reader_helper");
        let output = run_bounded_with_timeout(
            &std::env::current_exe().unwrap(),
            &["--exact", &helper, "--nocapture"],
            &[("SILO_TEST_DEPENDENCY_HASH_FIFO", path.as_path())],
            PROCESS_TIMEOUT,
        )
        .expect("dependency hashing must not wait for a FIFO writer");
        assert!(output.contains("1 passed"), "{output}");
    }

    #[test]
    fn dependency_hash_fifo_reader_helper() {
        let Some(path) = std::env::var_os("SILO_TEST_DEPENDENCY_HASH_FIFO") else {
            return;
        };
        assert!(matches!(
            sha256_file(Path::new(&path), Instant::now() + PROCESS_TIMEOUT),
            Err(ProbeError::Malformed(_))
        ));
    }

    #[test]
    fn fifo_manifest_reader_helper() {
        let Some(path) = std::env::var_os("SILO_TEST_MANIFEST_FIFO") else {
            return;
        };
        assert!(matches!(
            read_json::<serde_json::Value>(Path::new(&path)),
            Err(ProbeError::Malformed(_))
        ));
    }

    #[test]
    fn strict_manifests_reject_missing_integrity_evidence() {
        let input = r#"{"schemaVersion":1,"targetTriple":"x86_64-unknown-linux-gnu"}"#;
        assert!(serde_json::from_str::<GitManifest>(input).is_err());
    }

    #[test]
    fn missing_files_remain_unavailable() {
        let path = Path::new("/definitely/not/a/silo/runtime");
        assert!(matches!(readable_file(path), Err(ProbeError::Missing(_))));
        let check = readable_file(path)
            .unwrap_err()
            .to_check("runtime", "Runtime", true);
        assert_eq!(check.status, CheckStatus::Unavailable);
    }

    #[test]
    fn maps_actual_version_results_to_specific_states_and_captions() {
        assert_eq!(
            microsandbox_version_result(Ok("msb 0.7.6".into())),
            DependencyCheck::pass(
                "runtime-microsandbox",
                "MicroSandbox runtime",
                "Bundled msb 0.7.6 · libkrunfw 5.6.1"
            )
        );
        assert_eq!(
            git_version_result(Ok("git version 2.53.0".into()), "git version 2.53.0"),
            DependencyCheck::pass("tool-git", "Git", "Bundled Git 2.53.0")
        );
        assert_eq!(
            git_lfs_version_result(Ok("git-lfs/3.7.1 (GitHub; darwin arm64; go 1.24)".into())),
            DependencyCheck::pass("tool-git-lfs", "Git LFS", "Bundled Git LFS 3.7.1")
        );
        assert_eq!(
            microsandbox_version_result(Ok("msb 0.6.16".into())).status,
            CheckStatus::Failed
        );
        assert_eq!(
            git_lfs_version_result(Err(ProbeError::Timeout)).status,
            CheckStatus::Timeout
        );
    }

    #[test]
    fn rejects_manifest_filenames_outside_packaged_locations() {
        let target = expected_target().expect("tests run on a supported target");
        let (
            executable_name,
            executable_asset,
            executable_sha,
            library_name,
            library_asset,
            library_sha,
        ) = expected_runtime_assets(target).unwrap();
        let approved = serde_json::json!({
            "schemaVersion": 2,
            "microsandboxVersion": RUNTIME_INPUTS.microsandbox_version,
            "libkrunfwVersion": RUNTIME_INPUTS.libkrunfw_version,
            "targetTriple": target,
            "executable": {
                "bundledName": executable_name,
                "sha256": "1".repeat(64),
                "sourceCommit": RUNTIME_INPUTS.source_commit,
                "sourceArchiveSha256": RUNTIME_INPUTS.source_archive_sha256,
                "patchSha256s": RUNTIME_INPUTS.patches.iter().map(|patch| patch.sha256.clone()).collect::<Vec<_>>(),
                "toolchain": RUNTIME_INPUTS.toolchain,
                "features": RUNTIME_INPUTS.features,
                "officialReleaseAsset": executable_asset,
                "officialReleaseSha256": executable_sha,
                "embeddedAgentdReleaseAsset": expected_agentd_asset(target).unwrap().0,
                "embeddedAgentdReleaseSha256": expected_agentd_asset(target).unwrap().1
            },
            "library": { "bundledName": library_name, "releaseAsset": library_asset, "sha256": library_sha }
        });
        let mut runtime: MicrosandboxManifest = serde_json::from_value(approved.clone()).unwrap();
        assert!(runtime_manifest_matches(&runtime));
        for field in [
            "sourceCommit",
            "sourceArchiveSha256",
            "patchSha256s",
            "toolchain",
            "features",
            "officialReleaseSha256",
            "embeddedAgentdReleaseSha256",
        ] {
            let mut tampered = approved.clone();
            tampered["executable"][field] = if field == "patchSha256s" {
                serde_json::json!(["0".repeat(64)])
            } else {
                serde_json::json!("0".repeat(64))
            };
            let manifest: MicrosandboxManifest = serde_json::from_value(tampered).unwrap();
            assert!(
                !runtime_manifest_matches(&manifest),
                "accepted altered {field}"
            );
        }
        runtime.executable.bundled_name = "../../bin/sh".into();
        assert!(!runtime_manifest_matches(&runtime));
    }

    #[cfg(unix)]
    #[test]
    fn output_limit_stops_probes_before_they_continue_after_excessive_writes() {
        for script in [
            "printf '%65536s' x; printf finished > \"$1\"",
            "printf '%65536s' x >&2; printf finished > \"$1\"",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let marker = directory.path().join("finished");
            let result = run_bounded_with_timeout(
                Path::new("/bin/sh"),
                &["-c", script, "probe", marker.to_str().unwrap()],
                &[],
                PROCESS_TIMEOUT,
            );
            assert!(
                matches!(result, Err(ProbeError::Malformed(_))),
                "{result:?}"
            );
            assert!(
                !marker.exists(),
                "the oversized write completed before its limit was enforced"
            );
        }
    }

    #[test]
    fn expired_collection_deadline_does_not_start_another_probe() {
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("started");
        let result = run_bounded_until(
            Path::new("/bin/sh"),
            &[
                "-c",
                "printf started > \"$1\"",
                "probe",
                marker.to_str().unwrap(),
            ],
            &[],
            Instant::now(),
        );
        assert!(matches!(result, Err(ProbeError::Timeout)));
        assert!(!marker.exists());
    }

    #[test]
    fn probes_share_the_collection_deadline_and_keep_completed_results() {
        let deadline = Instant::now() + Duration::from_secs(1);
        let first = git_version_result(
            run_bounded_until(
                Path::new("/bin/sh"),
                &["-c", "printf 'git version 2.53.0'"],
                &[],
                deadline,
            ),
            "git version 2.53.0",
        );
        let second = git_lfs_version_result(run_bounded_until(
            Path::new("/bin/sh"),
            &["-c", "sleep 2; printf git-lfs/3.7.1"],
            &[],
            deadline,
        ));
        assert_eq!(first.status, CheckStatus::Pass);
        assert_eq!(second.status, CheckStatus::Timeout);
        assert_eq!(second.remediation.as_deref(), Some(RETRY_GUIDANCE));
    }

    #[test]
    fn kills_a_version_probe_at_its_deadline() {
        let started = Instant::now();
        let result = run_bounded_with_timeout(
            Path::new("/bin/sh"),
            &["-c", "sleep 2"],
            &[],
            Duration::from_millis(40),
        );
        assert!(matches!(result, Err(ProbeError::Timeout)));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    fn probe(id: &str, status: CheckStatus) -> DependencyCheck {
        match status {
            CheckStatus::Pass => DependencyCheck::pass(id, id, "ok"),
            status => DependencyCheck::failure(id, id, status, "x", RETRY_GUIDANCE),
        }
    }

    fn status_at(deadline: Instant) -> CheckStatus {
        if Instant::now() >= deadline {
            CheckStatus::Timeout
        } else {
            CheckStatus::Pass
        }
    }

    #[test]
    fn a_slow_group_does_not_starve_later_groups() {
        let report = run_groups(
            "r".into(),
            &[
                CheckGroup {
                    budget: Duration::from_millis(30),
                    run: Box::new(|deadline| {
                        std::thread::sleep(Duration::from_millis(80));
                        vec![probe("slow", status_at(deadline))]
                    }),
                },
                CheckGroup {
                    budget: Duration::from_secs(5),
                    run: Box::new(|deadline| vec![probe("later", status_at(deadline))]),
                },
            ],
        );
        assert_eq!(report.checks[0].status, CheckStatus::Timeout);
        assert_eq!(report.checks[1].status, CheckStatus::Pass);
    }

    #[test]
    fn a_timed_out_group_is_retried_once_and_only_once() {
        use std::cell::Cell;
        let calls = Cell::new(0);
        let report = run_groups(
            "r".into(),
            &[CheckGroup {
                budget: Duration::from_secs(1),
                run: Box::new(|_| {
                    calls.set(calls.get() + 1);
                    let status = if calls.get() == 1 {
                        CheckStatus::Timeout
                    } else {
                        CheckStatus::Pass
                    };
                    vec![probe("flaky", status)]
                }),
            }],
        );
        assert_eq!(calls.get(), 2);
        assert_eq!(report.checks[0].status, CheckStatus::Pass);

        let always = Cell::new(0);
        let report = run_groups(
            "r".into(),
            &[CheckGroup {
                budget: Duration::from_secs(1),
                run: Box::new(|_| {
                    always.set(always.get() + 1);
                    vec![probe("stuck", CheckStatus::Timeout)]
                }),
            }],
        );
        assert_eq!(always.get(), 2);
        assert_eq!(report.checks[0].status, CheckStatus::Timeout);
    }
}
