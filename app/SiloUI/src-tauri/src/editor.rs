//! Local editor handoff over MicroSandbox's SSH transport. Guest paths never
//! become host filesystem arguments. SSH keys and configuration stay on host.
use crate::{
    applications,
    runtime::{self, RuntimePaths},
};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

/// Serializes SSH key and configuration file writes only. Never hold it across
/// remote calls, guest commands, probes or editor launches: the desktop viewer
/// and remote authorization wait on it (G-22).
static LOCK: Mutex<()> = Mutex::new(());
const FAILED: &str = "Could not prepare the editor connection.";

/// The guard protects no in-memory state and every file write is atomic, so a
/// panic while holding it leaves nothing inconsistent; recover instead of
/// failing every editor connection until restart (G-23).
fn files_lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) fn open(app: &AppHandle, name: &str, path: Option<&str>) -> Result<(), String> {
    if let Some((device, computer)) = crate::remote_access::target(name)? {
        let path = path.unwrap_or("/workspace");
        let (alias, _) = prepare_remote(app, &device, &computer, path)?;
        let application = applications::selected_editor(app)?;
        let command = applications::editor_command(&application)?;
        let home = app.path().home_dir().map_err(|_| FAILED)?;
        return launch_editor(
            editor_launch(&command, &alias, path, &home)?,
            EDITOR_EXIT_WINDOW,
        );
    }
    require_openssh("open computer folders in your editor")?;
    runtime::validate_name(name).map_err(|error| error.to_string())?;
    let path = path.unwrap_or("/workspace");
    validate_path(path)?;
    let application = applications::selected_editor(app)?;
    let command = applications::editor_command(&application)?;
    let paths = runtime::runtime_paths(app)?;
    let metadata = runtime::read_metadata(&paths.metadata).map_err(|error| error.to_string())?;
    if !metadata
        .computers
        .iter()
        .any(|configuration| configuration.name() == name)
    {
        return Err("This computer does not support local editor connections.".into());
    }
    crate::terminal::running_computer(&paths, name)?;
    let user = crate::working_account::USER;
    // Validate the exact folder inside the guest, as positional data, before handoff.
    runtime::run_msb(
        &paths,
        &[
            "exec".into(),
            name.into(),
            "--no-start".into(),
            "--user".into(),
            user.into(),
            "--env".into(),
            format!("USER={user}"),
            "--env".into(),
            format!("LOGNAME={user}"),
            "--no-tty".into(),
            "--quiet".into(),
            "--timeout".into(),
            "5s".into(),
            "--".into(),
            "test".into(),
            "-d".into(),
            path.into(),
        ],
        Duration::from_secs(8),
    )
    .map_err(|_| "This folder is unavailable inside the computer.")?;
    let user_home = app.path().home_dir().map_err(|_| FAILED)?;
    let (alias, config) = prepare(&paths, &user_home, name)?;
    let mut probe = Command::new("/usr/bin/ssh");
    probe.args(["-F"]).arg(&config).args([
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=5",
        &alias,
        "true",
    ]);
    run(&mut probe, Duration::from_secs(10)).map_err(|_| {
        "Could not connect to this computer over SSH. Retry after checking that it is running."
    })?;
    launch_editor(
        editor_launch(&command, &alias, path, &user_home)?,
        EDITOR_EXIT_WINDOW,
    )
}

/// How long an editor launcher may take to report a failure. Launchers that
/// are still running then (an editor started without its CLI) are left
/// running and reaped in the background, never killed (G-06).
const EDITOR_EXIT_WINDOW: Duration = Duration::from_secs(10);

fn launch_editor(mut launch: Command, window: Duration) -> Result<(), String> {
    const FAILED_LAUNCH: &str =
        "The editor could not be opened. Check its installation and Remote SSH support.";
    let mut child = applications::launch::sanitize_child(&mut launch)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| FAILED_LAUNCH)?;
    let deadline = Instant::now() + window;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) | Err(_) => return Err(FAILED_LAUNCH.into()),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                return Ok(());
            }
        }
    }
}

/// Explains a missing system OpenSSH client instead of a generic failure.
pub(crate) fn require_openssh(purpose: &str) -> Result<(), String> {
    require_openssh_at(
        Path::new("/usr/bin/ssh"),
        Path::new("/usr/bin/ssh-keygen"),
        purpose,
    )
}
fn require_openssh_at(ssh: &Path, keygen: &Path, purpose: &str) -> Result<(), String> {
    if applications::launch::executable_file(ssh) && applications::launch::executable_file(keygen) {
        Ok(())
    } else {
        Err(format!(
            "OpenSSH is required to {purpose}. Install your system's OpenSSH client and retry."
        ))
    }
}

fn validate_path(path: &str) -> Result<(), String> {
    if path.len() > 4096
        || path.bytes().any(|byte| byte.is_ascii_control())
        || !(path == "/workspace"
            || path.strip_prefix("/workspace/").is_some_and(|tail| {
                tail.split('/')
                    .all(|part| !part.is_empty() && part != "." && part != "..")
            }))
    {
        return Err("Choose a folder inside /workspace with no control characters.".into());
    }
    Ok(())
}

/// VS Code profile for every Silo computer window, so the settings below and
/// the extensions that can reach the computer never mix with the user's own
/// profile (decision 5, G-19). VS Code creates it empty on first use and
/// offers to install Remote - SSH into it.
/// Carried by a Silo-owned workspace file on the device. Workspace settings
/// apply from the first window, whether or not the profile exists yet, and
/// outrank the "Remote" settings a computer can write for itself.
const VSCODE_SETTINGS: [(&str, bool); 4] = [
    // Git in the computer cannot borrow the GitHub session of VS Code on this device.
    ("github.gitAuthentication", false),
    // Computer terminals get no askpass handle back to VS Code on this device.
    ("git.terminalAuthentication", false),
    // Computer ports reach this device only through Silo's port publishing.
    ("remote.autoForwardPorts", false),
    ("remote.forwardOnOpen", false),
];

/// The editor command for a computer folder: Zed receives its SSH URI; VS Code
/// opens the Silo profile with the folder's Silo workspace file.
fn editor_launch(
    command: &applications::launch::EditorCommand,
    alias: &str,
    path: &str,
    user_home: &Path,
) -> Result<Command, String> {
    let mut launch = Command::new(&command.program);
    launch.args(&command.args);
    if command.zed {
        launch.arg(remote_uri(alias, path, true)?);
    } else {
        launch
            .args(["--profile", crate::channel::current().vscode_profile()])
            .arg(vscode_workspace(
                &crate::channel::current().state_dir(user_home),
                alias,
                path,
            )?);
    }
    Ok(launch)
}

/// Writes `<channel home>/editor/<alias>/<path hash>/<folder>.code-workspace`, keeping
/// any other workspace settings the user added and restoring Silo's own.
fn vscode_workspace(silo_root: &Path, alias: &str, path: &str) -> Result<PathBuf, String> {
    use sha2::{Digest, Sha256};
    validate_path(path)?;
    if alias.is_empty()
        || !alias
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(FAILED.into());
    }
    runtime::prepare_private_directory(silo_root).map_err(|_| FAILED)?;
    let mut directory = silo_root.join("editor");
    private_directory(&directory)?;
    directory.push(alias);
    private_directory(&directory)?;
    directory.push(&format!("{:x}", Sha256::digest(path.as_bytes()))[..12]);
    private_directory(&directory)?;
    let folder: String = path
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || " -_.".contains(character) {
                character
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    let folder = folder.trim_start_matches('.').trim();
    let file = directory.join(format!(
        "{}.code-workspace",
        if folder.is_empty() {
            "workspace"
        } else {
            folder
        }
    ));
    let old = read_regular(&file)?;
    let invalid = || {
        format!(
            "Silo cannot update the editor workspace {} as a JSON object. The file was left unchanged. Remove comments or repair its JSON, then retry.",
            file.display()
        )
    };
    let mut document = if old.is_empty() && !file.exists() {
        serde_json::json!({})
    } else {
        serde_json::from_slice::<serde_json::Value>(&old).map_err(|_| invalid())?
    };
    if !document.is_object() {
        return Err(invalid());
    }
    document["folders"] = serde_json::json!([{ "uri": remote_uri(alias, path, false)? }]);
    document["remoteAuthority"] = serde_json::json!(format!("ssh-remote+{alias}"));
    if !document["settings"].is_object() {
        document["settings"] = serde_json::json!({});
    }
    for (key, value) in VSCODE_SETTINGS {
        document["settings"][key] = serde_json::json!(value);
    }
    let bytes = serde_json::to_vec_pretty(&document).map_err(|_| FAILED)?;
    write_private(&file, &bytes)?;
    Ok(file)
}

fn remote_uri(alias: &str, path: &str, zed: bool) -> Result<String, String> {
    validate_path(path)?;
    let mut uri = reqwest::Url::parse(&if zed {
        format!("ssh://{alias}/")
    } else {
        format!("vscode-remote://ssh-remote+{alias}/")
    })
    .map_err(|_| FAILED)?;
    uri.path_segments_mut()
        .map_err(|_| FAILED)?
        .clear()
        .extend(path.split('/').skip(1));
    Ok(uri.into())
}

pub(crate) fn private_directory(path: &Path) -> Result<(), String> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(FAILED.into());
        }
    }
    fs::create_dir_all(path).map_err(|_| FAILED)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|_| FAILED.into())
}

pub(crate) fn read_regular(path: &Path) -> Result<Vec<u8>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.len() <= 1024 * 1024 =>
        {
            fs::read(path).map_err(|_| FAILED.into())
        }
        Ok(_) => Err(
            "The SSH configuration cannot be updated safely. Check its file type and size.".into(),
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(_) => Err(FAILED.into()),
    }
}

pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    read_regular(path)?;
    let parent = path.parent().ok_or(FAILED)?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|_| FAILED)?;
    file.as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|_| FAILED)?;
    file.write_all(bytes).map_err(|_| FAILED)?;
    file.as_file().sync_all().map_err(|_| FAILED)?;
    file.persist(path).map_err(|_| FAILED)?;
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| FAILED)?;
    Ok(())
}

pub(crate) fn key(path: &Path) -> Result<(), String> {
    let bytes = read_regular(path)?;
    if !bytes.is_empty() {
        let metadata = fs::symlink_metadata(path).map_err(|_| FAILED)?;
        if metadata.permissions().mode() & 0o077 != 0 {
            write_private(path, &bytes)?;
        }
        return Ok(());
    }
    let mut command = Command::new("/usr/bin/ssh-keygen");
    command
        .args([
            "-q",
            "-t",
            "ed25519",
            "-N",
            "",
            "-C",
            crate::channel::current().product_name(),
            "-f",
        ])
        .arg(path);
    run(&mut command, Duration::from_secs(5))
}

pub(crate) fn public_key(path: &Path) -> Result<String, String> {
    let mut command = Command::new("/usr/bin/ssh-keygen");
    command.args(["-y", "-f"]).arg(path);
    public_key_from_command(&mut command, Duration::from_secs(5))
}

fn public_key_from_command(command: &mut Command, timeout: Duration) -> Result<String, String> {
    let mut output = tempfile::tempfile().map_err(|_| FAILED)?;
    run_with_stdout(
        command,
        timeout,
        Stdio::from(output.try_clone().map_err(|_| FAILED)?),
    )?;
    output.seek(SeekFrom::Start(0)).map_err(|_| FAILED)?;
    let mut text = String::new();
    output
        .take(4097)
        .read_to_string(&mut text)
        .map_err(|_| FAILED)?;
    if text.len() > 4096 {
        return Err(FAILED.into());
    }
    let mut fields = text.split_whitespace();
    let kind = fields.next().ok_or(FAILED)?;
    let value = fields.next().ok_or(FAILED)?;
    if kind != "ssh-ed25519" || value.len() > 256 {
        return Err(FAILED.into());
    }
    Ok(format!("{kind} {value}"))
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
fn ssh_quote(path: &Path) -> Result<String, String> {
    let text = path.to_str().ok_or(FAILED)?;
    if text.chars().any(char::is_control) {
        return Err(FAILED.into());
    }
    Ok(format!(
        "\"{}\"",
        text.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
    ))
}

fn prepare(
    paths: &RuntimePaths,
    user_home: &Path,
    name: &str,
) -> Result<(String, PathBuf), String> {
    let root = paths.home.join("ssh");
    let config = root.join(format!("{name}.conf"));
    let known_hosts = root.join(format!("{name}.known_hosts"));
    let alias = prepare_configuration(paths, name, &config, &known_hosts)?;
    let _guard = files_lock();
    install_include(user_home, &include_line(&root)?)?;
    Ok((alias, config))
}

/// The `Include` that makes the entries in `root` visible to the user's `ssh`.
fn include_line(root: &Path) -> Result<String, String> {
    let mut pattern = String::new();
    for character in root.to_str().ok_or(FAILED)?.chars() {
        if matches!(character, '\\' | '[' | ']' | '?' | '*') {
            pattern.push('\\');
        }
        pattern.push(character);
    }
    Ok(format!(
        "Include {}",
        ssh_quote(&Path::new(&pattern).join("*.conf"))?
    ))
}

fn owned(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    metadata.uid() == unsafe { libc::geteuid() }
}

fn manual_include(config: &Path, include: &str) -> String {
    format!(
        "Silo can't safely update {}, which links to a file it can't change. Add this line at the top of that file, then try again: {include}",
        config.display()
    )
}

fn has_line(contents: &[u8], line: &str) -> bool {
    contents
        .split(|byte| *byte == b'\n')
        .any(|current| current == line.as_bytes())
}

/// Prepends Silo's `Include` to the user's SSH configuration once. Links from
/// dotfile managers (stow, chezmoi) are followed when they lead to a folder or
/// file this account owns; otherwise, such as a read-only home-manager file,
/// the user gets the exact line to add (G-11).
fn install_include(user_home: &Path, include: &str) -> Result<(), String> {
    let link = user_home.join(".ssh");
    let user_config = link.join("config");
    let manual = || manual_include(&user_config, include);
    let ssh_root = match fs::symlink_metadata(&link) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let target = fs::canonicalize(&link).map_err(|_| manual())?;
            match fs::metadata(&target) {
                Ok(metadata) if metadata.is_dir() && owned(&metadata) => target,
                _ => return Err(manual()),
            }
        }
        _ => {
            private_directory(&link)?;
            link.clone()
        }
    };
    let config = ssh_root.join("config");
    if !fs::symlink_metadata(&config).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        let old = read_regular(&config)?;
        if !has_line(&old, include) {
            let mut new = format!("{include}\n").into_bytes();
            new.extend_from_slice(&old);
            write_private(&config, &new)?;
        }
        return Ok(());
    }
    let target = fs::canonicalize(&config).map_err(|_| manual())?;
    let old = read_regular(&target)?;
    if has_line(&old, include) {
        return Ok(());
    }
    let parent_owned = target
        .parent()
        .and_then(|parent| fs::metadata(parent).ok())
        .is_some_and(|metadata| owned(&metadata));
    if !parent_owned
        || !fs::metadata(&target).is_ok_and(|metadata| metadata.is_file() && owned(&metadata))
    {
        return Err(manual());
    }
    let mut new = format!("{include}\n").into_bytes();
    new.extend_from_slice(&old);
    // Replacing the resolved file keeps the user's link in place.
    replace_file(&target, &new).map_err(|_| manual())
}

/// Atomically replaces a regular file and keeps its permission bits.
fn replace_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let mode = fs::metadata(path)?.mode() & 0o666;
    let parent = path.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.as_file()
        .set_permissions(fs::Permissions::from_mode(mode))?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|error| error.error)?;
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

/// Private connections share the editor's host-only identity, without installing
/// an Include in the user's SSH configuration or changing editor connection files.
pub(crate) fn prepare_private_transport(
    paths: &RuntimePaths,
    name: &str,
    directory: &Path,
) -> Result<(String, PathBuf), String> {
    runtime::validate_name(name).map_err(|error| error.to_string())?;
    private_directory(directory)?;
    let config = directory.join("ssh_config");
    let known_hosts = directory.join("known_hosts");
    let alias = prepare_configuration(paths, name, &config, &known_hosts)?;
    Ok((alias, config))
}

/// The SSH alias and private configuration for a local or remote computer, in a
/// per-purpose directory below the runtime's `ssh` folder. Nothing is added to the
/// user's own SSH configuration.
pub(crate) fn private_computer_transport(
    app: &AppHandle,
    computer: &str,
    purpose: &str,
) -> Result<(String, PathBuf), String> {
    if let Some((device, remote)) = crate::remote_access::target(computer)? {
        return prepare_remote_private(app, &device, &remote, "/workspace");
    }
    runtime::validate_name(computer).map_err(|error| error.to_string())?;
    let paths = runtime::runtime_paths(app)?;
    let directory = paths.home.join("ssh").join(purpose).join(computer);
    prepare_private_transport(&paths, computer, &directory)
}

fn prepare_configuration(
    paths: &RuntimePaths,
    name: &str,
    config: &Path,
    known_hosts: &Path,
) -> Result<String, String> {
    let user = crate::working_account::inspect_user(paths, name)?;
    let _guard = files_lock();
    let root = paths.home.join("ssh");
    private_directory(&root)?;
    let client = root.join("silo_ed25519");
    key(&client)?;
    let authorized = root.join("authorized_keys");
    let public = public_key(&client)?;
    let mut contents = read_regular(&authorized)?;
    let text = std::str::from_utf8(&contents).map_err(|_| FAILED)?;
    if !text.lines().any(|line| line == public) {
        if !contents.is_empty() && !contents.ends_with(b"\n") {
            contents.push(b'\n');
        }
        contents.extend_from_slice(format!("{public}\n").as_bytes());
        write_private(&authorized, &contents)?;
    }
    let host_root = paths.home.join("sandboxes").join(name).join("ssh");
    private_directory(&host_root)?;
    let host_key = host_root.join("host_ed25519");
    key(&host_key)?;
    let suffix = paths
        .home
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(FAILED)?;
    let alias = format!("silo-{suffix}-{name}");
    write_private(
        &known_hosts,
        format!("{alias} {}\n", public_key(&host_key)?).as_bytes(),
    )?;
    let proxy = local_proxy(paths, name)?;
    let hosts = host_patterns(config, &alias, name);
    let content = format!("Host {hosts}\n  HostName {alias}\n  User {user}\n  IdentityFile {}\n  IdentitiesOnly yes\n  IdentityAgent none\n  ForwardAgent no\n  ForwardX11 no\n  UserKnownHostsFile {}\n  StrictHostKeyChecking yes\n  BatchMode yes\n  ProxyCommand {proxy}\n\nHost *\n", ssh_quote(&client)?, ssh_quote(&known_hosts)?);
    write_private(&config, content.as_bytes())?;
    Ok(alias)
}

/// The `Host` line of an entry: `alias`, then the aliases an earlier runtime home
/// gave this computer that the entry already answers to. An editor that saved one
/// of them (the storage migration changes the alias) keeps finding the computer
/// instead of falling back to a copy kept from before the upgrade.
fn host_patterns(config: &Path, alias: &str, name: &str) -> String {
    let earlier = read_regular(config)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| Some(text.lines().next()?.strip_prefix("Host ")?.to_owned()))
        .unwrap_or_default();
    let mut patterns = vec![alias];
    for pattern in earlier.split_whitespace() {
        let hash = pattern
            .strip_prefix("silo-")
            .and_then(|rest| rest.strip_suffix(name))
            .and_then(|rest| rest.strip_suffix('-'));
        if hash.is_some_and(|hash| !hash.is_empty() && hash.bytes().all(|b| b.is_ascii_hexdigit()))
            && !patterns.contains(&pattern)
        {
            patterns.push(pattern);
        }
    }
    patterns.join(" ")
}

fn run(command: &mut Command, timeout: Duration) -> Result<(), String> {
    run_with_stdout(command, timeout, Stdio::null())
}

fn run_with_stdout(command: &mut Command, timeout: Duration, stdout: Stdio) -> Result<(), String> {
    let mut child = applications::launch::sanitize_child(command)
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| FAILED)?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err(FAILED.into())
                }
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(FAILED.into());
            }
        }
    }
}

pub(crate) fn validate_public_key(public: &str) -> Result<(), String> {
    use base64::Engine;
    let mut parts = public.split(' ');
    if parts.next() != Some("ssh-ed25519") {
        return Err("Expected an Ed25519 public key.".into());
    }
    let encoded = parts.next().ok_or("Missing public key.")?;
    if parts.next().is_some() || encoded.len() > 128 {
        return Err("Invalid public key.".into());
    }
    let data = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| "Invalid public key.")?;
    if data.len() != 51 || &data[..19] != b"\x00\x00\x00\x0bssh-ed25519\x00\x00\x00\x20" {
        return Err("Invalid Ed25519 public key.".into());
    }
    Ok(())
}

pub(crate) fn authorize_remote(
    paths: &RuntimePaths,
    name: &str,
    public: &str,
    path: &str,
) -> Result<String, String> {
    crate::runtime::shutdown::ensure_accepting_operations()?;
    validate_public_key(public)?;
    validate_path(path)?;
    crate::terminal::running_computer(paths, name)?;
    let user = crate::working_account::USER;
    runtime::run_msb(
        paths,
        &[
            "exec".into(),
            name.into(),
            "--no-start".into(),
            "--user".into(),
            user.into(),
            "--env".into(),
            format!("USER={user}"),
            "--env".into(),
            format!("LOGNAME={user}"),
            "--no-tty".into(),
            "--quiet".into(),
            "--timeout".into(),
            "5s".into(),
            "--".into(),
            "test".into(),
            "-d".into(),
            path.into(),
        ],
        Duration::from_secs(8),
    )
    .map_err(|_| "This folder is unavailable inside the computer.")?;
    let _guard = files_lock();
    let root = paths.home.join("ssh");
    private_directory(&root)?;
    let authorized = root.join("authorized_keys");
    let mut contents = read_regular(&authorized)?;
    if !std::str::from_utf8(&contents)
        .map_err(|_| FAILED)?
        .lines()
        .any(|line| line == public)
    {
        if !contents.is_empty() && !contents.ends_with(b"\n") {
            contents.push(b'\n');
        }
        contents.extend_from_slice(format!("{public}\n").as_bytes());
        write_private(&authorized, &contents)?;
    }
    let host_root = paths.home.join("sandboxes").join(name).join("ssh");
    private_directory(&host_root)?;
    let host_key = host_root.join("host_ed25519");
    key(&host_key)?;
    public_key(&host_key)
}

/// Prepare the pinned guest SSH identity without changing the user's SSH configuration.
pub(crate) fn prepare_remote_private(
    app: &AppHandle,
    device: &str,
    computer: &str,
    path: &str,
) -> Result<(String, PathBuf), String> {
    prepare_remote_transport(app, device, computer, path, None, false)
        .map(|(alias, config, _)| (alias, config))
}
pub(crate) fn prepare_remote_network_private(
    app: &AppHandle,
    device: &str,
    computer: &str,
    port: u16,
) -> Result<(String, PathBuf, std::net::IpAddr), String> {
    let (alias, config, address) =
        prepare_remote_transport(app, device, computer, "/workspace", Some(port), true)?;
    Ok((
        alias,
        config,
        address.ok_or("Guest network address is unavailable.")?,
    ))
}
fn prepare_remote_transport(
    app: &AppHandle,
    device: &str,
    computer: &str,
    path: &str,
    port: Option<u16>,
    forwarding: bool,
) -> Result<(String, PathBuf, Option<std::net::IpAddr>), String> {
    validate_path(path)?;
    uuid::Uuid::parse_str(device).map_err(|_| "Invalid device identity.")?;
    uuid::Uuid::parse_str(computer).map_err(|_| "Invalid computer identity.")?;
    let home = app.path().home_dir().map_err(|_| FAILED)?;
    let state = crate::channel::current().state_dir(&home);
    crate::runtime::prepare_private_directory(&state).map_err(|e| e.to_string())?;
    let root = state.join("desktop-remote/ssh");
    let client = root.join(format!("{device}.key"));
    let client_public = {
        let _guard = files_lock();
        private_directory(&root)?;
        key(&client)?;
        public_key(&client)?
    };
    // The remote call can take minutes; keep the file lock free meanwhile.
    let (host_public, user, address) =
        crate::remote_access::prepare(app, device, computer, &client_public, path, forwarding)?;
    let _guard = files_lock();
    let alias = format!(
        "{}-{device}-{computer}",
        crate::channel::current().remote_alias_prefix()
    );
    let known_hosts = root.join(format!("{device}-{computer}.known_hosts"));
    write_private(&known_hosts, format!("{alias} {host_public}\n").as_bytes())?;
    // A published-port forward has its own configuration: its ProxyCommand names the
    // guest port the owner revokes by, and tunnels for other ports run concurrently.
    let proxy = remote_proxy_for(device, computer, port)?;
    let config = match port {
        Some(port) => root.join(format!("{device}-{computer}-port-{port}.conf")),
        None => root.join(format!("{device}-{computer}.conf")),
    };
    let contents = format!("Host {alias}\n  HostName {alias}\n  User {user}\n  IdentityFile {}\n  IdentitiesOnly yes\n  IdentityAgent none\n  ForwardAgent no\n  ForwardX11 no\n  UserKnownHostsFile {}\n  StrictHostKeyChecking yes\n  BatchMode yes\n  ProxyCommand {proxy}\n\nHost *\n", ssh_quote(&client)?, ssh_quote(&known_hosts)?);
    write_private(&config, contents.as_bytes())?;
    Ok((alias, config, address))
}

fn proxy_command<S: AsRef<str>>(parts: &[S]) -> String {
    parts
        .iter()
        .map(|part| quote(&part.as_ref().replace('%', "%%")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The ProxyCommand for a local computer. An AppImage's bundled tools live in a
/// mount that changes on every start, so editor configurations that outlive
/// Silo call the AppImage file itself instead (G-12).
fn local_proxy(paths: &RuntimePaths, name: &str) -> Result<String, String> {
    runtime::validate_name(name).map_err(|error| error.to_string())?;
    let home = paths.home.to_str().ok_or(FAILED)?;
    if applications::launch::tools_are_temporary() {
        let silo = applications::launch::stable_executable()?;
        return Ok(proxy_command(&[
            silo.to_str().ok_or(FAILED)?,
            TRANSPORT_MODE,
            home,
            name,
        ]));
    }
    let executable = paths.executable.to_str().ok_or(FAILED)?;
    Ok(proxy_command(&[
        "/usr/bin/env",
        &format!("MSB_HOME={home}"),
        &format!("MSB_PATH={executable}"),
        &format!(
            "MSB_LIBKRUNFW_PATH={}",
            paths.library.to_str().ok_or(FAILED)?
        ),
        executable,
        "ssh",
        "serve",
        name,
        "--stdio",
        "--no-start",
        "--no-inactivity-timeout",
    ]))
}

/// The ProxyCommand for a computer on another device, through this Silo.
fn remote_proxy(device: &str, computer: &str) -> Result<String, String> {
    remote_proxy_for(device, computer, None)
}

/// The same, for a published-port forward when `port` is the guest port.
fn remote_proxy_for(device: &str, computer: &str, port: Option<u16>) -> Result<String, String> {
    uuid::Uuid::parse_str(device).map_err(|_| FAILED)?;
    uuid::Uuid::parse_str(computer).map_err(|_| FAILED)?;
    let silo = applications::launch::stable_executable()?;
    let port = port.map(|port| port.to_string());
    let mut parts = vec![
        silo.to_str().ok_or(FAILED)?,
        "--remote-guest",
        device,
        computer,
    ];
    parts.extend(port.as_deref());
    Ok(proxy_command(&parts))
}

/// `silo --msb-ssh-serve <runtime home> <computer>`: the local editor transport
/// run from an AppImage, whose bundled runtime has no stable path (G-12).
pub(crate) const TRANSPORT_MODE: &str = "--msb-ssh-serve";

/// Runs the bundled `msb ssh serve` in this AppImage's mount for an editor.
pub(crate) fn run_transport(args: &[String]) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    let [home, name] = args else {
        return Err("Expected a runtime home and a computer name.".into());
    };
    runtime::validate_name(name).map_err(|error| error.to_string())?;
    let home = Path::new(home);
    if !home.is_absolute() {
        return Err("Expected an absolute runtime home.".into());
    }
    let executable = std::env::current_exe().map_err(|_| FAILED)?;
    let bundle = tauri::utils::platform::bundle_type();
    let appimage = std::env::var_os("APPDIR").map(PathBuf::from);
    let msb = crate::bundled_tools::resolve(
        &executable,
        Path::new(""),
        bundle.clone(),
        appimage.as_deref(),
    )?
    .join("msb");
    let library = runtime::bundled_runtime_library(&msb, Path::new(""), bundle);
    let error = Command::new(&msb)
        .args([
            "ssh",
            "serve",
            name,
            "--stdio",
            "--no-start",
            "--no-inactivity-timeout",
        ])
        .env("MSB_HOME", home)
        .env("MSB_PATH", &msb)
        .env("MSB_LIBKRUNFW_PATH", &library)
        .exec();
    Err(format!("Could not start the computer connection: {error}"))
}

/// Replaces the value of every `directive` line of a configuration Silo wrote.
fn with_directive(contents: &str, directive: &str, value: &str) -> Option<String> {
    let prefix = format!("  {directive} ");
    let mut found = false;
    let lines: Vec<String> = contents
        .split('\n')
        .map(|line| {
            if line.starts_with(&prefix) {
                found = true;
                format!("{prefix}{value}")
            } else {
                line.to_owned()
            }
        })
        .collect();
    found.then(|| lines.join("\n"))
}

/// Replaces the ProxyCommand of a configuration Silo wrote.
fn with_proxy(contents: &str, proxy: &str) -> Option<String> {
    with_directive(contents, "ProxyCommand", proxy)
}

/// Rewrites every `*.conf` Silo wrote in `root`, where `rewrite` maps a file stem
/// and its contents to the new contents, or `None` to leave the file alone.
fn rewrite_configs(
    root: &Path,
    rewrite: &dyn Fn(&str, &str) -> Option<String>,
) -> Result<(), String> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => {
            return Err(format!(
                "Could not read editor configurations in {}.",
                root.display()
            ))
        }
    };
    let mut failures = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                failures.push(format!(
                    "Could not read an editor configuration in {}.",
                    root.display()
                ));
                continue;
            }
        };
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "conf") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            failures.push(format!(
                "Invalid editor configuration name in {}.",
                root.display()
            ));
            continue;
        };
        let result = (|| {
            let bytes = read_regular(&path)?;
            let contents = String::from_utf8(bytes)
                .map_err(|_| "The SSH configuration is not UTF-8.".to_owned())?;
            if let Some(updated) = rewrite(stem, &contents).filter(|updated| *updated != contents) {
                write_private(&path, updated.as_bytes())?;
            }
            Ok::<_, String>(())
        })();
        if let Err(error) = result {
            failures.push(format!("{}: {error}", path.display()));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Some editor configurations could not be repaired. {}",
            failures.join(" ")
        ))
    }
}

/// Rewrites the ProxyCommand of every `*.conf` Silo wrote in `root`, where
/// `proxy_for` maps a file stem to its current command.
fn refresh_configs(root: &Path, proxy_for: &dyn Fn(&str) -> Option<String>) -> Result<(), String> {
    rewrite_configs(root, &|stem, contents| {
        with_proxy(contents, &proxy_for(stem)?)
    })
}

/// Points a local entry written for another runtime home at `paths`: the runtime
/// its ProxyCommand starts and the identity and known-hosts files it names. The
/// `Host` name stays, so an editor that saved it still finds the entry.
fn with_local_entry(contents: &str, paths: &RuntimePaths, name: &str) -> Option<String> {
    let root = paths.home.join("ssh");
    let mut updated = with_proxy(contents, &local_proxy(paths, name).ok()?)?;
    for (directive, file) in [
        ("IdentityFile", root.join("silo_ed25519")),
        (
            "UserKnownHostsFile",
            root.join(format!("{name}.known_hosts")),
        ),
    ] {
        if let Some(next) = with_directive(&updated, directive, &ssh_quote(&file).ok()?) {
            updated = next;
        }
    }
    Some(updated)
}

/// Whether the user's SSH configuration, read through any link, has this line.
fn user_config_has_line(user_home: &Path, line: &str) -> bool {
    let path = user_home.join(".ssh/config");
    fs::metadata(&path)
        .ok()
        .filter(|metadata| metadata.is_file() && metadata.len() <= 1024 * 1024)
        .and_then(|_| fs::read(&path).ok())
        .is_some_and(|contents| has_line(&contents, line))
}

/// The storage migration copies the previous generation's editor entries as they
/// were, so they still start the previous runtime home: an editor that reconnects
/// by itself would run `msb ssh serve` against the pre-upgrade copy of the computer,
/// outside the migration guard, and against nothing once that backup is deleted.
/// Points every entry in the converted home at the converted home, and adds its
/// `Include` where the user's SSH configuration still includes the previous one.
///
/// The previous home is never written. The old `Include` line stays: the new one
/// goes first and `ssh` keeps the first value it finds, so the repointed entries
/// win, a glob that matches nothing is harmless, and the user's file keeps
/// everything but the one prepended line. Safe to repeat. Reports failed entry
/// rewrites before adding the `Include`.
fn repoint_converted_entries(
    paths: &RuntimePaths,
    user_home: &Path,
    previous_home: &Path,
) -> Result<(), String> {
    let root = paths.home.join("ssh");
    let _guard = files_lock();
    rewrite_configs(&root, &|name, contents| {
        with_local_entry(contents, paths, name)
    })?;
    if !user_config_has_line(user_home, &include_line(&previous_home.join("ssh"))?) {
        return Ok(());
    }
    install_include(user_home, &include_line(&root)?)
}

/// What a launch's repair of the editor entries left for the user.
#[derive(Debug, PartialEq)]
enum Repair {
    /// The entries point at the converted home, and nothing is left to do.
    Done,
    /// Silo can't change the user's SSH configuration (the manual line
    /// `install_include` explains): this `Include` line is for the user to add.
    ManualInclude(String),
    /// Anything else that went wrong, which may pass on its own.
    Failed(String),
}

/// `repoint_converted_entries`, told apart by what the user can do about its failure.
fn repair_after_migration(paths: &RuntimePaths, user_home: &Path, previous_home: &Path) -> Repair {
    let Err(error) = repoint_converted_entries(paths, user_home, previous_home) else {
        return Repair::Done;
    };
    match include_line(&paths.home.join("ssh")) {
        Ok(line) if error == manual_include(&user_home.join(".ssh/config"), &line) => {
            Repair::ManualInclude(line)
        }
        _ => Repair::Failed(error),
    }
}

/// The `Include` line the last repair could not add and the user has to. The
/// notice in the application shows it until the line is in the user's SSH
/// configuration or the user dismisses it, so a missed toast never loses it.
static MANUAL_INCLUDE: Mutex<Option<String>> = Mutex::new(None);
const MANUAL_INCLUDE_CHANGED: &str = "silo://editor-include-changed";

/// Replaces the line the user has to add. Whether that changed it.
fn replace_manual_include(line: Option<String>) -> bool {
    let mut current = MANUAL_INCLUDE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let changed = *current != line;
    *current = line;
    changed
}

/// Tells the application when the line the user has to add changed.
fn set_manual_include(app: &AppHandle, line: Option<String>) {
    if replace_manual_include(line) {
        let _ = app.emit(MANUAL_INCLUDE_CHANGED, ());
    }
}

/// The line the user has to add, unless their SSH configuration has it by now.
fn manual_include_needed(user_home: &Path, line: Option<String>) -> Option<String> {
    line.filter(|line| !user_config_has_line(user_home, line))
}

/// The `Include` line the user must add to their SSH configuration themselves
/// because Silo can't change it, or `None`. Checked against the file at every
/// read, so a line added since is no longer reported.
#[tauri::command]
pub(crate) async fn read_editor_include_notice(
    app: AppHandle,
    window: WebviewWindow,
) -> Result<Option<String>, String> {
    if window.label() != "main" {
        return Err("Only the main window can read the editor connection notice.".into());
    }
    let home = app.path().home_dir().map_err(|_| FAILED)?;
    let line = MANUAL_INCLUDE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    tauri::async_runtime::spawn_blocking(move || manual_include_needed(&home, line))
        .await
        .map_err(|_| FAILED.into())
}

/// The runtime home the previous generation used, once the storage migration
/// completed and converted every computer. `None` before that, without any
/// migration, and after "Continue" into a fresh runtime, where the previous
/// generation holds the only copy of the unconverted computers.
fn previous_home_after_migration(app_data: &Path, user_home: &Path) -> Option<PathBuf> {
    let locations = crate::runtime_migration::backup_locations(app_data)?;
    Some(runtime::runtime_home_alias(
        user_home,
        &locations.previous.join("microsandbox"),
    ))
}

/// At startup, points editor configurations at the runtime that holds their
/// computers. After a storage migration that converted every computer (any build,
/// every launch, so installs migrated by an earlier Silo are repaired too) the
/// entries copied from the previous generation name its runtime home. Under an
/// AppImage, also points configurations written by an earlier run at the
/// AppImage file instead of that run's mount (G-12, with C-19). Editors
/// reconnecting after a restart then find the computer and the transport.
pub(crate) fn refresh_transports(app: &AppHandle) {
    let appimage = applications::launch::tools_are_temporary();
    let migrated = app
        .path()
        .app_data_dir()
        .ok()
        .zip(app.path().home_dir().ok())
        .and_then(|(app_data, home)| {
            Some((previous_home_after_migration(&app_data, &home)?, home))
        });
    if !appimage && migrated.is_none() {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        if let (Some((previous_home, home)), Ok(paths)) = (migrated, runtime::runtime_paths(&app)) {
            match repair_after_migration(&paths, &home, &previous_home) {
                Repair::Done => set_manual_include(&app, None),
                Repair::ManualInclude(line) => set_manual_include(&app, Some(line)),
                Repair::Failed(error) => {
                    set_manual_include(&app, None);
                    crate::notifications::notify(
                        &app,
                        crate::notifications::failure(
                            "startup:editor-connections",
                            "Editor connections need attention",
                            &format!("Editors that reconnect on their own may still open the computer copies kept from before the upgrade. {error}"),
                            None,
                        ),
                    );
                }
            }
        }
        if !appimage {
            return;
        }
        let _guard = files_lock();
        let mut failures = Vec::new();
        if let Ok(paths) = runtime::runtime_paths(&app) {
            if let Err(error) = refresh_configs(&paths.home.join("ssh"), &|name| {
                local_proxy(&paths, name).ok()
            }) {
                failures.push(error);
            }
        }
        if let Ok(home) = app.path().home_dir() {
            if let Err(error) = refresh_configs(
                &crate::channel::current()
                    .state_dir(&home)
                    .join("desktop-remote/ssh"),
                &|stem| {
                    let (device, computer) = (stem.get(..36)?, stem.get(37..)?);
                    (stem.as_bytes().get(36) == Some(&b'-')).then_some(())?;
                    remote_proxy(device, computer).ok()
                },
            ) {
                failures.push(error);
            }
        }
        drop(_guard);
        if !failures.is_empty() {
            crate::notifications::notify(
                &app,
                crate::notifications::failure(
                    "startup:editor-connections",
                    "Editor connections need attention",
                    &failures.join(" "),
                    None,
                ),
            );
        }
    });
}

pub(crate) fn prepare_remote(
    app: &AppHandle,
    device: &str,
    computer: &str,
    path: &str,
) -> Result<(String, PathBuf), String> {
    let (alias, config) = prepare_remote_private(app, device, computer, path)?;
    let root = config.parent().ok_or(FAILED)?;
    let home = app.path().home_dir().map_err(|_| FAILED)?;
    let _guard = files_lock();
    install_include(&home, &include_line(root)?)?;
    Ok((alias, config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_file_writers_report_an_unreadable_parent_after_replacement() {
        use std::os::unix::fs::MetadataExt;
        for private in [true, false] {
            let directory = tempfile::tempdir().unwrap();
            if fs::metadata(directory.path()).unwrap().uid() == 0 {
                return; // Root bypasses the permission boundary exercised here.
            }
            let path = directory.path().join("config");
            fs::write(&path, b"previous SSH configuration").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
            fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o300)).unwrap();
            let result = if private {
                write_private(&path, b"complete replacement").map_err(std::io::Error::other)
            } else {
                replace_file(&path, b"complete replacement")
            };
            fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
            assert_eq!(fs::read(&path).unwrap(), b"complete replacement");
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                if private { 0o600 } else { 0o640 }
            );
            assert!(
                result.is_err(),
                "an unsynchronized rename must not report success"
            );
            assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
        }
    }
    #[test]
    fn a_panic_while_writing_ssh_files_does_not_disable_editor_connections() {
        let _ = std::thread::spawn(|| {
            let _guard = files_lock();
            panic!("simulated failure while holding the SSH file lock");
        })
        .join();
        assert!(LOCK.is_poisoned());
        drop(files_lock());
        let dir = tempfile::tempdir().unwrap();
        let _guard = files_lock();
        private_directory(&dir.path().join("ssh")).unwrap();
    }

    #[test]
    fn a_missing_openssh_client_is_explained() {
        let dir = tempfile::tempdir().unwrap();
        let ssh = dir.path().join("ssh");
        let keygen = dir.path().join("ssh-keygen");
        let error = require_openssh_at(&ssh, &keygen, "view computer desktops").unwrap_err();
        assert_eq!(error, "OpenSSH is required to view computer desktops. Install your system's OpenSSH client and retry.");
        fs::write(&ssh, b"").unwrap();
        assert!(require_openssh_at(&ssh, &keygen, "view computer desktops").is_err());
        fs::write(&keygen, b"").unwrap();
        assert!(require_openssh_at(&ssh, &keygen, "view computer desktops").is_err());
        fs::set_permissions(&ssh, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(require_openssh_at(&ssh, &keygen, "view computer desktops").is_err());
        fs::set_permissions(&keygen, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(require_openssh_at(&ssh, &keygen, "view computer desktops").is_ok());
    }

    #[test]
    fn public_key_output_from_a_stalled_helper_is_rejected() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "printf 'ssh-ed25519 test'; exec /bin/sleep 0.2"]);
        assert!(public_key_from_command(&mut command, Duration::from_millis(20)).is_err());
    }

    #[test]
    fn public_key_helper_requires_successful_completion() {
        for (script, expected) in [
            ("printf 'ssh-ed25519 test'", Some("ssh-ed25519 test")),
            ("printf 'ssh-ed25519 test'; exit 1", None),
        ] {
            let mut command = Command::new("/bin/sh");
            command.args(["-c", script]);
            assert_eq!(
                public_key_from_command(&mut command, Duration::from_secs(1))
                    .ok()
                    .as_deref(),
                expected
            );
        }
    }

    #[test]
    fn generated_key_comment_names_the_channel_and_existing_keys_are_preserved() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("id_ed25519");
        key(&path).unwrap();
        let public = fs::read_to_string(dir.path().join("id_ed25519.pub")).unwrap();
        assert!(public.ends_with(&format!(" {}\n", crate::channel::current().product_name())));
        let private = fs::read(&path).unwrap();
        key(&path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), private);
        assert_eq!(
            fs::read_to_string(dir.path().join("id_ed25519.pub")).unwrap(),
            public
        );
    }

    #[test]
    fn remote_authorization_accepts_only_plain_ed25519_public_keys() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("controller");
        key(&path).unwrap();
        let public = public_key(&path).unwrap();
        validate_public_key(&public).unwrap();
        for invalid in [
            format!("command=evil {public}"),
            format!("{public}\n{public}"),
            format!("{public} comment"),
            "ssh-ed25519 YQ==".into(),
        ] {
            assert!(validate_public_key(&invalid).is_err());
        }
    }
    #[test]
    fn editor_handoff_keeps_percent_names_and_encoded_dot_segments_literal() {
        let directory = tempfile::tempdir().unwrap();
        for (path, encoded) in [
            ("/workspace/a%2Fb", "/workspace/a%252Fb"),
            ("/workspace/%2e%2e/outside", "/workspace/%252e%252e/outside"),
            ("/workspace/%2E./outside", "/workspace/%252E./outside"),
            ("/workspace/100% done", "/workspace/100%25%20done"),
        ] {
            validate_path(path).unwrap();
            for zed in [false, true] {
                let uri =
                    reqwest::Url::parse(&remote_uri("silo-test-dev", path, zed).unwrap()).unwrap();
                assert_eq!(uri.path(), encoded, "{path:?}, zed={zed}");
            }
            let file = vscode_workspace(directory.path(), "silo-test-dev", path).unwrap();
            let document: serde_json::Value =
                serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
            assert_eq!(
                document["folders"][0]["uri"],
                format!("vscode-remote://ssh-remote+silo-test-dev{encoded}"),
            );
        }
        for path in ["/workspace/a\nb", "/workspace/a\rb", "/workspace/a\tb"] {
            for zed in [false, true] {
                assert!(remote_uri("silo-test-dev", path, zed).is_err());
            }
            assert!(vscode_workspace(directory.path(), "silo-test-dev", path).is_err());
        }
    }

    #[test]
    fn remote_paths_stay_in_uri_and_are_encoded() {
        let uri = remote_uri("silo-test-dev", "/workspace/a b/#test?x", true).unwrap();
        assert_eq!(uri, "ssh://silo-test-dev/workspace/a%20b/%23test%3Fx");
        assert!(remote_uri("silo-test-dev", "/workspace", false)
            .unwrap()
            .starts_with("vscode-remote://ssh-remote+silo-test-dev/"));
        for invalid in ["/tmp", "/workspace/../tmp", "/workspace/a\0"] {
            assert!(validate_path(invalid).is_err());
        }
    }
    #[test]
    fn control_characters_cannot_change_the_requested_editor_folder() {
        for path in ["/workspace/a\tb", "/workspace/a\nb", "/workspace/a\rb"] {
            assert!(validate_path(path).is_err());
            assert!(remote_uri("silo-test-dev", path, true).is_err());
            assert!(remote_uri("silo-test-dev", path, false).is_err());
        }
    }

    #[test]
    fn literal_percent_sequences_keep_the_guest_folder_identity() {
        for (path, encoded) in [
            ("/workspace/some%20comments", "/workspace/some%2520comments"),
            ("/workspace/a%2Fb", "/workspace/a%252Fb"),
            ("/workspace/%2e%2e/secret", "/workspace/%252e%252e/secret"),
            ("/workspace/100%", "/workspace/100%25"),
        ] {
            validate_path(path).unwrap();
            for (zed, prefix) in [
                (true, "ssh://silo-test-dev"),
                (false, "vscode-remote://ssh-remote+silo-test-dev"),
            ] {
                assert_eq!(
                    remote_uri("silo-test-dev", path, zed).unwrap(),
                    format!("{prefix}{encoded}")
                );
            }
        }
    }

    #[test]
    fn ssh_includes_keep_wildcard_characters_in_directory_names_literal() {
        let home = tempfile::tempdir().unwrap();
        for (index, name) in ["home[1]", "home?", "home*", "home\\folder"]
            .into_iter()
            .enumerate()
        {
            let root = home.path().join(name).join("ssh");
            fs::create_dir_all(&root).unwrap();
            let hostname = format!("selected-computer-{index}");
            fs::write(
                root.join("dev.conf"),
                format!("Host silo-test-dev\n  HostName {hostname}\n"),
            )
            .unwrap();
            let config = home.path().join("config");
            fs::write(&config, format!("{}\n", include_line(&root).unwrap())).unwrap();
            let output = Command::new("/usr/bin/ssh")
                .args(["-G", "-F"])
                .arg(&config)
                .arg("silo-test-dev")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{name}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                String::from_utf8(output.stdout)
                    .unwrap()
                    .contains(&format!("hostname {hostname}\n")),
                "{name}: the Include must read the exact directory"
            );
        }
    }

    #[test]
    fn shell_and_ssh_paths_are_escaped() {
        assert_eq!(quote("a'b $()"), "'a'\\''b $()'");
        assert_eq!(ssh_quote(Path::new("/a%b\"c")).unwrap(), "\"/a%%b\\\"c\"");
    }
    #[test]
    fn private_writes_preserve_bytes_and_refuse_symlinks() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("config");
        write_private(&file, b"Host existing\n  User user\n").unwrap();
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let link = directory.path().join("link");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert!(write_private(&link, b"replace").is_err());
        assert_eq!(fs::read(&file).unwrap(), b"Host existing\n  User user\n");
    }

    #[test]
    fn reused_ssh_keys_repair_broad_permissions_without_rotating_identity() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("client");
        key(&file).unwrap();
        let private = fs::read(&file).unwrap();
        let public = public_key(&file).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o666)).unwrap();
        key(&file).unwrap();
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(fs::read(&file).unwrap(), private);
        assert_eq!(public_key(&file).unwrap(), public);
    }

    #[test]
    fn editor_configurations_get_a_fresh_proxy_command_at_startup() {
        let root = tempfile::tempdir().unwrap();
        let config = "Host silo-abc-dev\n  HostName silo-abc-dev\n  User silo\n  ProxyCommand '/tmp/.mount_old/usr/libexec/silo/tools/msb' 'ssh' 'serve' 'dev'\n\nHost *\n";
        fs::write(root.path().join("dev.conf"), config).unwrap();
        fs::write(root.path().join("dev.known_hosts"), "unchanged").unwrap();
        fs::write(root.path().join("bad name.conf"), config).unwrap();
        refresh_configs(root.path(), &|name| {
            runtime::validate_name(name).ok()?;
            Some(proxy_command(&[
                "/home/me/Silo.AppImage",
                TRANSPORT_MODE,
                "/home/me/.silo/abc",
                name,
            ]))
        })
        .unwrap();
        let updated = fs::read_to_string(root.path().join("dev.conf")).unwrap();
        assert_eq!(
            updated,
            config.replace(
                "'/tmp/.mount_old/usr/libexec/silo/tools/msb' 'ssh' 'serve' 'dev'",
                "'/home/me/Silo.AppImage' '--msb-ssh-serve' '/home/me/.silo/abc' 'dev'"
            )
        );
        assert_eq!(
            fs::read_to_string(root.path().join("bad name.conf")).unwrap(),
            config
        );
        assert_eq!(
            fs::read_to_string(root.path().join("dev.known_hosts")).unwrap(),
            "unchanged"
        );
        assert_eq!(with_proxy("Host x\n", "p"), None);
    }

    #[test]
    fn proxy_commands_outside_an_appimage_keep_their_direct_form() {
        let paths = RuntimePaths {
            guest_image: PathBuf::new(),
            executable: "/Applications/Silo.app/Contents/MacOS/msb".into(),
            home: "/Users/me/.silo/abc".into(),
            storage_home: None,
            library: "/Applications/Silo.app/Contents/Frameworks/libkrunfw.5.dylib".into(),
            metadata: PathBuf::new(),
            volumes: PathBuf::new(),
        };
        assert_eq!(
            local_proxy(&paths, "dev").unwrap(),
            "'/usr/bin/env' 'MSB_HOME=/Users/me/.silo/abc' 'MSB_PATH=/Applications/Silo.app/Contents/MacOS/msb' 'MSB_LIBKRUNFW_PATH=/Applications/Silo.app/Contents/Frameworks/libkrunfw.5.dylib' '/Applications/Silo.app/Contents/MacOS/msb' 'ssh' 'serve' 'dev' '--stdio' '--no-start' '--no-inactivity-timeout'"
        );
        let device = "0b6a1c9e-9f55-4d8e-9d2c-3f0a4b5c6d7e";
        let computer = "1c7b2d0f-0a66-4e9f-8e3d-4a1b5c6d7e8f";
        let remote = remote_proxy(device, computer).unwrap();
        assert!(remote.ends_with(&format!("'--remote-guest' '{device}' '{computer}'")));
        assert!(remote_proxy("not-a-uuid", computer).is_err());
        assert!(remote_proxy_for(device, computer, Some(3000))
            .unwrap()
            .ends_with(&format!("'--remote-guest' '{device}' '{computer}' '3000'")));
        assert!(run_transport(&["relative".into(), "dev".into()]).is_err());
        assert!(run_transport(&["/home".into(), "bad;name".into()]).is_err());
    }

    fn vscode(program: &str) -> applications::launch::EditorCommand {
        applications::launch::EditorCommand {
            program: program.into(),
            args: Vec::new(),
            zed: false,
        }
    }

    #[test]
    fn an_editor_that_keeps_running_is_left_open_and_a_failed_launch_is_reported() {
        let started = Instant::now();
        let mut long = Command::new("/bin/sh");
        long.args(["-c", "echo $$ > \"$0\"; exec sleep 30"]);
        let directory = tempfile::tempdir().unwrap();
        let pid_file = directory.path().join("editor.pid");
        long.arg(&pid_file);
        launch_editor(long, Duration::from_millis(300)).unwrap();
        assert!(started.elapsed() < Duration::from_secs(5));
        let pid: i32 = fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(
            unsafe { libc::kill(pid, 0) },
            0,
            "the editor must not be killed"
        );
        unsafe { libc::kill(pid, libc::SIGTERM) };
        assert!(launch_editor(Command::new("/usr/bin/false"), Duration::from_secs(5)).is_err());
        assert!(launch_editor(Command::new("/usr/bin/true"), Duration::from_secs(5)).is_ok());
    }

    fn launch_args(launch: &Command) -> Vec<String> {
        launch
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn vscode_opens_the_silo_profile_with_protective_computer_settings() {
        let home = tempfile::tempdir().unwrap();
        let code = vscode("/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code");
        let launch =
            editor_launch(&code, "silo-abc-dev", "/workspace/my repo", home.path()).unwrap();
        assert_eq!(launch.get_program(), code.program.as_os_str());
        let args = launch_args(&launch);
        assert_eq!(args[..2], ["--profile", "Silo"]);
        assert_eq!(
            args.len(),
            3,
            "no --folder-uri: the workspace file names the folder"
        );
        let file = PathBuf::from(&args[2]);
        assert!(file.starts_with(home.path().join(".silo/editor/silo-abc-dev")));
        assert_eq!(file.file_name().unwrap(), "my repo.code-workspace");
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(file.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        let document: serde_json::Value =
            serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        assert_eq!(
            document["folders"],
            serde_json::json!([{ "uri": "vscode-remote://ssh-remote+silo-abc-dev/workspace/my%20repo" }])
        );
        assert_eq!(document["remoteAuthority"], "ssh-remote+silo-abc-dev");
        assert_eq!(
            document["settings"],
            serde_json::json!({
                "github.gitAuthentication": false,
                "git.terminalAuthentication": false,
                "remote.autoForwardPorts": false,
                "remote.forwardOnOpen": false,
            })
        );
        // Another folder of the same computer gets its own workspace file.
        let other = editor_launch(&code, "silo-abc-dev", "/workspace", home.path()).unwrap();
        assert_ne!(launch_args(&other)[2], args[2]);
        assert!(launch_args(&other)[2].ends_with("/workspace.code-workspace"));
    }

    #[test]
    fn user_computer_settings_survive_while_silo_settings_are_restored() {
        let home = tempfile::tempdir().unwrap();
        let code = vscode("/usr/bin/code");
        let args =
            launch_args(&editor_launch(&code, "silo-abc-dev", "/workspace", home.path()).unwrap());
        fs::write(
            &args[2],
            br#"{"folders":[{"uri":"file:///elsewhere"}],"settings":{"editor.fontSize":15,"remote.autoForwardPorts":true}}"#,
        )
        .unwrap();
        editor_launch(&code, "silo-abc-dev", "/workspace", home.path()).unwrap();
        let document: serde_json::Value =
            serde_json::from_slice(&fs::read(&args[2]).unwrap()).unwrap();
        assert_eq!(document["settings"]["editor.fontSize"], 15);
        assert_eq!(document["settings"]["remote.autoForwardPorts"], false);
        assert_eq!(
            document["folders"][0]["uri"],
            "vscode-remote://ssh-remote+silo-abc-dev/workspace"
        );
    }

    #[test]
    fn an_unparseable_workspace_file_is_preserved_instead_of_replaced() {
        let home = tempfile::tempdir().unwrap();
        let root = crate::channel::current().state_dir(home.path());
        let file = vscode_workspace(&root, "silo-abc-dev", "/workspace").unwrap();
        for contents in [
            "{\n// keep my workspace settings\n\"settings\": {\"editor.fontSize\": 15}}",
            "{\"settings\":",
            "[]",
            "",
        ] {
            fs::write(&file, contents).unwrap();
            let error = vscode_workspace(&root, "silo-abc-dev", "/workspace").unwrap_err();
            assert!(error.contains("workspace"));
            assert!(error.contains("unchanged"));
            assert_eq!(fs::read_to_string(&file).unwrap(), contents);
        }
    }

    #[test]
    fn zed_keeps_its_ssh_uri_and_no_workspace_file() {
        let home = tempfile::tempdir().unwrap();
        let zed = applications::launch::EditorCommand {
            program: "/usr/bin/flatpak".into(),
            args: vec!["run".into(), "dev.zed.Zed".into()],
            zed: true,
        };
        let launch = editor_launch(&zed, "silo-abc-dev", "/workspace", home.path()).unwrap();
        assert_eq!(
            launch_args(&launch),
            ["run", "dev.zed.Zed", "ssh://silo-abc-dev/workspace"]
        );
        assert!(!home.path().join(".silo").exists());
    }

    const INCLUDE: &str = "Include \"/home/user/.silo/abc/ssh/*.conf\"";

    #[test]
    fn ssh_includes_treat_runtime_directory_names_as_literal_paths() {
        let home = tempfile::tempdir().unwrap();
        for name in ["ssh[fixture]", "ssh?fixture", "ssh*fixture", "ssh\\fixture"] {
            let root = home.path().join(name);
            fs::create_dir_all(&root).unwrap();
            fs::write(
                root.join("dev.conf"),
                "Host silo-glob-fixture\n  HostName 127.0.0.9\n",
            )
            .unwrap();
            let config = home.path().join("fixture.conf");
            fs::write(&config, format!("{}\n", include_line(&root).unwrap())).unwrap();
            let output = Command::new("/usr/bin/ssh")
                .env("HOME", home.path())
                .args(["-G", "-F"])
                .arg(&config)
                .arg("silo-glob-fixture")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{name}: SSH configuration parsing failed"
            );
            assert!(
                String::from_utf8(output.stdout)
                    .unwrap()
                    .lines()
                    .any(|line| line == "hostname 127.0.0.9"),
                "{name}: the included host was not found"
            );
        }
    }

    #[test]
    fn a_stow_linked_ssh_config_is_updated_through_its_link() {
        let home = tempfile::tempdir().unwrap();
        let dotfiles = home.path().join("dotfiles/ssh");
        fs::create_dir_all(&dotfiles).unwrap();
        fs::write(dotfiles.join("config"), b"Host personal\n  User me\n").unwrap();
        fs::set_permissions(dotfiles.join("config"), fs::Permissions::from_mode(0o644)).unwrap();
        private_directory(&home.path().join(".ssh")).unwrap();
        std::os::unix::fs::symlink("../dotfiles/ssh/config", home.path().join(".ssh/config"))
            .unwrap();
        install_include(home.path(), INCLUDE).unwrap();
        install_include(home.path(), INCLUDE).unwrap();
        let link = home.path().join(".ssh/config");
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the link stays"
        );
        assert_eq!(
            fs::read(&link).unwrap(),
            format!("{INCLUDE}\nHost personal\n  User me\n").as_bytes()
        );
        assert_eq!(
            fs::metadata(&link).unwrap().permissions().mode() & 0o777,
            0o644
        );
    }

    #[test]
    fn a_linked_ssh_folder_receives_a_new_config_without_replacing_the_link() {
        let home = tempfile::tempdir().unwrap();
        let dotfiles = home.path().join("dotfiles/ssh");
        fs::create_dir_all(&dotfiles).unwrap();
        std::os::unix::fs::symlink(&dotfiles, home.path().join(".ssh")).unwrap();
        install_include(home.path(), INCLUDE).unwrap();
        assert!(fs::symlink_metadata(home.path().join(".ssh"))
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            fs::read(dotfiles.join("config")).unwrap(),
            format!("{INCLUDE}\n").as_bytes()
        );
    }

    #[test]
    fn an_unwritable_linked_config_explains_the_line_to_add() {
        let home = tempfile::tempdir().unwrap();
        let store = home.path().join("store");
        fs::create_dir_all(&store).unwrap();
        fs::write(store.join("config"), b"Host managed\n").unwrap();
        private_directory(&home.path().join(".ssh")).unwrap();
        std::os::unix::fs::symlink(store.join("config"), home.path().join(".ssh/config")).unwrap();
        // Like a read-only home-manager file in the Nix store: read-only for
        // this account, or owned by someone else when the tests run as root.
        let root = unsafe { libc::geteuid() } == 0;
        let lock = |locked: bool| {
            if root {
                let owner = if locked { 65534 } else { 0 };
                std::os::unix::fs::chown(&store, Some(owner), None).unwrap();
                std::os::unix::fs::chown(store.join("config"), Some(owner), None).unwrap();
            } else {
                fs::set_permissions(
                    &store,
                    fs::Permissions::from_mode(if locked { 0o555 } else { 0o755 }),
                )
                .unwrap();
            }
        };
        lock(true);
        let error = install_include(home.path(), INCLUDE).unwrap_err();
        assert!(
            error.ends_with(&format!(
                "Add this line at the top of that file, then try again: {INCLUDE}"
            )),
            "{error}"
        );
        assert_eq!(fs::read(store.join("config")).unwrap(), b"Host managed\n");
        // Once the user adds the line, nothing needs to be written.
        lock(false);
        fs::write(store.join("config"), format!("{INCLUDE}\nHost managed\n")).unwrap();
        lock(true);
        install_include(home.path(), INCLUDE).unwrap();
        lock(false);
    }

    #[test]
    fn a_dangling_config_link_is_not_replaced() {
        let home = tempfile::tempdir().unwrap();
        private_directory(&home.path().join(".ssh")).unwrap();
        std::os::unix::fs::symlink("/silo-test-missing/config", home.path().join(".ssh/config"))
            .unwrap();
        assert!(install_include(home.path(), INCLUDE)
            .unwrap_err()
            .contains(INCLUDE));
        assert!(fs::symlink_metadata(home.path().join(".ssh/config"))
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[test]
    fn ssh_configuration_preserves_user_content_and_is_idempotent() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().join("user");
        private_directory(&home.join(".ssh")).unwrap();
        let existing =
            b"# personal settings\nServerAliveInterval 37\nHost personal\n  User example\n";
        fs::write(home.join(".ssh/config"), existing).unwrap();
        let paths = RuntimePaths {
            guest_image: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("runtime/guest-image"),
            executable: directory.path().join("msb"),
            home: directory.path().join("runtime"),
            storage_home: None,
            library: directory.path().join("msb"),
            metadata: directory.path().join("computers.json"),
            volumes: directory.path().join("volumes"),
        };
        crate::working_account::test_runtime(&paths.executable);
        let (alias, config) = prepare(&paths, &home, "dev").unwrap();
        let once = fs::read(home.join(".ssh/config")).unwrap();
        prepare(&paths, &home, "dev").unwrap();
        assert_eq!(once, fs::read(home.join(".ssh/config")).unwrap());
        assert!(once.ends_with(existing));
        let output = Command::new("/usr/bin/ssh")
            .arg("-G")
            .arg("-F")
            .arg(config)
            .arg(alias)
            .output()
            .unwrap();
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("user silo\n"));
        assert!(text.contains("stricthostkeychecking true"));
        assert!(text.contains("identityagent none"));
        assert!(text.contains("forwardagent no"));
        assert!(text.contains("--no-start"));
        let personal = Command::new("/usr/bin/ssh")
            .arg("-G")
            .arg("-F")
            .arg(home.join(".ssh/config"))
            .arg("personal")
            .output()
            .unwrap();
        assert!(personal.status.success());
        let personal = String::from_utf8(personal.stdout).unwrap();
        assert!(personal.contains("user example\n"));
        assert!(!personal.contains("--no-start"));
        assert!(personal.contains("serveraliveinterval 37\n"));
        let unrelated = Command::new("/usr/bin/ssh")
            .arg("-G")
            .arg("-F")
            .arg(home.join(".ssh/config"))
            .arg("unrelated")
            .output()
            .unwrap();
        assert!(unrelated.status.success());
        assert!(String::from_utf8(unrelated.stdout)
            .unwrap()
            .contains("serveraliveinterval 37\n"));
    }

    /// A device after the storage migration: the editor entry `prepare` wrote for
    /// the previous runtime home (and the `Include` it added), and the converted
    /// home holding the verbatim copy the migration made of that home.
    struct Migrated {
        directory: tempfile::TempDir,
        user_home: PathBuf,
        previous: RuntimePaths,
        converted: RuntimePaths,
    }

    const OLD_HOME: &str = "aaaaaaaaaaaa";
    const NEW_HOME: &str = "bbbbbbbbbbbb";
    const OLD_DEVICE: &str = "silo-aaaaaaaaaaaa-dev";

    fn migrated() -> Migrated {
        let directory = tempfile::tempdir().unwrap();
        let user_home = directory.path().join("user");
        private_directory(&user_home.join(".ssh")).unwrap();
        fs::write(
            user_home.join(".ssh/config"),
            b"# personal settings\nServerAliveInterval 37\nHost personal\n  User example\n",
        )
        .unwrap();
        let paths = |home: &str, executable: &str| RuntimePaths {
            guest_image: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("runtime/guest-image"),
            executable: directory.path().join(executable),
            home: user_home.join(".silo").join(home),
            storage_home: None,
            library: directory.path().join(executable),
            metadata: directory.path().join("computers.json"),
            volumes: directory.path().join("volumes"),
        };
        let previous = paths(OLD_HOME, "old-msb");
        let converted = paths(NEW_HOME, "new-msb");
        crate::working_account::test_runtime(&previous.executable);
        prepare(&previous, &user_home, "dev").unwrap();
        fs::create_dir_all(&converted.home).unwrap();
        let copy = Command::new("/bin/cp")
            .arg("-R")
            .arg(previous.home.join("."))
            .arg(&converted.home)
            .status()
            .unwrap();
        assert!(copy.success());
        Migrated {
            directory,
            user_home,
            previous,
            converted,
        }
    }

    impl Migrated {
        fn config(&self) -> Vec<u8> {
            fs::read(self.user_home.join(".ssh/config")).unwrap()
        }

        fn entry(&self, home: &RuntimePaths) -> String {
            fs::read_to_string(home.home.join("ssh/dev.conf")).unwrap()
        }

        fn repoint(&self) -> Result<(), String> {
            repoint_converted_entries(&self.converted, &self.user_home, &self.previous.home)
        }

        /// What `ssh` resolves for `alias` from the user's own configuration.
        fn resolved(&self, alias: &str) -> String {
            let output = Command::new("/usr/bin/ssh")
                .arg("-G")
                .arg("-F")
                .arg(self.user_home.join(".ssh/config"))
                .arg(alias)
                .output()
                .unwrap();
            assert!(output.status.success());
            String::from_utf8(output.stdout).unwrap()
        }
    }

    /// Every file under `root` with its bytes and permissions.
    fn snapshot(root: &Path) -> std::collections::BTreeMap<PathBuf, (Vec<u8>, u32)> {
        use std::os::unix::fs::MetadataExt;
        let mut files = std::collections::BTreeMap::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(directory) = stack.pop() {
            for entry in fs::read_dir(&directory).unwrap() {
                let path = entry.unwrap().path();
                let metadata = fs::symlink_metadata(&path).unwrap();
                if metadata.is_dir() {
                    stack.push(path);
                } else {
                    files.insert(path.clone(), (fs::read(&path).unwrap(), metadata.mode()));
                }
            }
        }
        files
    }

    #[test]
    fn entries_copied_by_a_migration_are_pointed_at_the_converted_home() {
        use std::os::unix::fs::MetadataExt;
        let device = migrated();
        let include = |home: &str| {
            format!(
                "Include \"{}/.silo/{home}/ssh/*.conf\"",
                device.user_home.display()
            )
        };
        // Before: the copy and the user's file still lead to the previous home.
        let before = device.resolved(OLD_DEVICE);
        assert!(before.contains(&format!("MSB_HOME={}", device.previous.home.display())));
        assert!(!before.contains(NEW_HOME));
        let previous_before = snapshot(&device.previous.home);
        let config_before = device.config();
        assert!(config_before.starts_with(include(OLD_HOME).as_bytes()));

        device.repoint().unwrap();

        // The Host name an editor saved still resolves, now to the converted home:
        // the proxy, the identity and the known hosts it names are all there.
        // The previous `Include` is still there, but it comes after the new one and
        // `ssh` keeps the first value it finds, so the proxy and known hosts are the
        // converted home's; the identity files are cumulative and the converted one is first.
        let after = device.resolved(OLD_DEVICE);
        let converted = device.converted.home.display().to_string();
        let values = |key: &str| -> Vec<String> {
            after
                .lines()
                .filter_map(|line| line.strip_prefix(&format!("{key} ")))
                .map(str::to_owned)
                .collect()
        };
        let proxy = values("proxycommand");
        assert_eq!(proxy.len(), 1, "{after}");
        assert!(
            proxy[0].contains(&format!("MSB_HOME={converted}")),
            "{after}"
        );
        assert!(!proxy[0].contains(&device.previous.home.display().to_string()));
        assert!(proxy[0].contains(&format!(
            "MSB_PATH={}",
            device.converted.executable.display()
        )));
        assert_eq!(
            values("identityfile")[0],
            format!("{converted}/ssh/silo_ed25519")
        );
        assert_eq!(
            values("userknownhostsfile"),
            [format!("{converted}/ssh/dev.known_hosts")]
        );
        assert_eq!(values("hostname"), [OLD_DEVICE]);
        assert_eq!(values("stricthostkeychecking"), ["true"]);
        let entry = device.entry(&device.converted);
        assert!(entry.starts_with(&format!(
            "Host {OLD_DEVICE}\n  HostName {OLD_DEVICE}\n  User silo\n"
        )));
        assert!(entry.ends_with("\n\nHost *\n"));
        assert_eq!(
            fs::metadata(device.converted.home.join("ssh/dev.conf"))
                .unwrap()
                .mode()
                & 0o777,
            0o600
        );

        // The user's file gains one line at the top; the previous line and the rest stay.
        let config = device.config();
        assert_eq!(
            config,
            [
                format!("{}\n", include(NEW_HOME)).as_bytes(),
                &config_before[..]
            ]
            .concat()
        );
        // The unrelated entries of the user keep resolving as before.
        assert!(device.resolved("personal").contains("user example\n"));
        assert!(device
            .resolved("unrelated")
            .contains("serveraliveinterval 37\n"));

        // The backup stays an exact copy: nothing in the previous home was written.
        assert_eq!(snapshot(&device.previous.home), previous_before);

        // Repeating changes nothing, anywhere.
        let converted_before = snapshot(&device.converted.home);
        device.repoint().unwrap();
        assert_eq!(device.config(), config);
        assert_eq!(snapshot(&device.converted.home), converted_before);
        assert_eq!(snapshot(&device.previous.home), previous_before);
        drop(device.directory);
    }

    #[test]
    fn the_converted_entry_does_not_depend_on_the_previous_home_once_it_is_deleted() {
        let device = migrated();
        device.repoint().unwrap();
        fs::remove_dir_all(&device.previous.home).unwrap();
        let entry = device.entry(&device.converted);
        assert!(!entry.contains(&device.previous.home.display().to_string()));
        let resolved = device.resolved(OLD_DEVICE);
        assert!(resolved.contains(&format!("MSB_HOME={}", device.converted.home.display())));
        assert!(!resolved.contains("old-msb"));
        // The key the entry names is still there to offer.
        assert!(device.converted.home.join("ssh/silo_ed25519").is_file());
    }

    #[test]
    fn a_user_who_opened_the_computer_after_the_migration_already_has_the_include() {
        let device = migrated();
        // `prepare` for the converted home writes its own entry and `Include` first.
        crate::working_account::test_runtime(&device.converted.executable);
        prepare(&device.converted, &device.user_home, "dev").unwrap();
        let config = device.config();
        let entry = device.entry(&device.converted);
        device.repoint().unwrap();
        assert_eq!(device.config(), config);
        assert_eq!(device.entry(&device.converted), entry);
    }

    #[test]
    fn opening_the_computer_again_keeps_the_alias_an_editor_saved_before_the_upgrade() {
        let device = migrated();
        device.repoint().unwrap();
        crate::working_account::test_runtime(&device.converted.executable);
        prepare(&device.converted, &device.user_home, "dev").unwrap();
        let entry = device.entry(&device.converted);
        let new_device = format!("silo-{NEW_HOME}-dev");
        assert!(
            entry.starts_with(&format!(
                "Host {new_device} {OLD_DEVICE}\n  HostName {new_device}\n"
            )),
            "{entry}"
        );
        // Both names reach the converted computer; the copy kept from before the
        // upgrade is shadowed by the new `Include` for either of them.
        for alias in [new_device.as_str(), OLD_DEVICE] {
            let resolved = device.resolved(alias);
            let proxy: Vec<_> = resolved
                .lines()
                .filter(|line| line.starts_with("proxycommand "))
                .collect();
            assert_eq!(proxy.len(), 1, "{resolved}");
            assert!(proxy[0].contains(&format!("MSB_HOME={}", device.converted.home.display())));
            assert!(resolved.contains(&format!("hostname {new_device}\n")));
        }
        // Repeating changes nothing.
        prepare(&device.converted, &device.user_home, "dev").unwrap();
        assert_eq!(device.entry(&device.converted), entry);
    }

    #[test]
    fn only_aliases_silo_gave_this_computer_are_kept_in_a_host_line() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("dev.conf");
        let patterns = |first_line: &str| {
            fs::write(&config, format!("{first_line}\n  HostName x\n")).unwrap();
            host_patterns(&config, "silo-bbbbbbbbbbbb-dev", "dev")
        };
        assert_eq!(
            patterns("Host silo-aaaaaaaaaaaa-dev"),
            "silo-bbbbbbbbbbbb-dev silo-aaaaaaaaaaaa-dev"
        );
        assert_eq!(
            patterns("Host silo-bbbbbbbbbbbb-dev silo-aaaaaaaaaaaa-dev silo-cccccccccccc-dev"),
            "silo-bbbbbbbbbbbb-dev silo-aaaaaaaaaaaa-dev silo-cccccccccccc-dev"
        );
        for foreign in [
            "Host silo-aaaaaaaaaaaa-other",
            "Host silo-aaaaaaaaaaaa-x-dev",
            "Host silo-zzzz-dev",
            "Host silo--dev",
            "Host personal *",
            "Host",
            "",
        ] {
            assert_eq!(patterns(foreign), "silo-bbbbbbbbbbbb-dev", "{foreign}");
        }
        assert_eq!(
            host_patterns(
                &directory.path().join("missing.conf"),
                "silo-bbbbbbbbbbbb-dev",
                "dev"
            ),
            "silo-bbbbbbbbbbbb-dev"
        );
    }

    #[test]
    fn the_include_is_added_only_where_the_user_included_the_previous_home() {
        let device = migrated();
        // The user removed Silo's line from their configuration on their own.
        let plain = b"Host personal\n  User example\n";
        fs::write(device.user_home.join(".ssh/config"), plain).unwrap();
        device.repoint().unwrap();
        assert_eq!(device.config(), plain);
        // The copy is still pointed at the converted home.
        assert!(device
            .entry(&device.converted)
            .contains(&format!("MSB_HOME={}", device.converted.home.display())));
        // No SSH configuration at all: none is created.
        fs::remove_file(device.user_home.join(".ssh/config")).unwrap();
        device.repoint().unwrap();
        assert!(!device.user_home.join(".ssh/config").exists());
    }

    #[test]
    fn a_stow_linked_config_receives_the_include_through_its_link() {
        let device = migrated();
        let dotfiles = device.directory.path().join("dotfiles");
        fs::create_dir_all(&dotfiles).unwrap();
        fs::rename(
            device.user_home.join(".ssh/config"),
            dotfiles.join("ssh-config"),
        )
        .unwrap();
        let link = device.user_home.join(".ssh/config");
        std::os::unix::fs::symlink(dotfiles.join("ssh-config"), &link).unwrap();
        let before = fs::read(&link).unwrap();
        device.repoint().unwrap();
        device.repoint().unwrap();
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            fs::read(&link).unwrap(),
            [
                format!(
                    "Include \"{}/.silo/{NEW_HOME}/ssh/*.conf\"\n",
                    device.user_home.display()
                )
                .as_bytes(),
                &before[..]
            ]
            .concat()
        );
        assert!(device
            .resolved(OLD_DEVICE)
            .contains(&format!("MSB_HOME={}", device.converted.home.display())));
    }

    #[test]
    fn an_unwritable_linked_config_explains_the_line_to_add_and_the_entries_are_still_repointed() {
        let device = migrated();
        let (store, lock) = read_only_linked_config(&device);
        let before = fs::read(store.join("config")).unwrap();
        lock(true);
        let error = device.repoint().unwrap_err();
        lock(false);
        let include = format!(
            "Include \"{}/.silo/{NEW_HOME}/ssh/*.conf\"",
            device.user_home.display()
        );
        assert!(
            error.ends_with(&format!(
                "Add this line at the top of that file, then try again: {include}"
            )),
            "{error}"
        );
        assert_eq!(fs::read(store.join("config")).unwrap(), before);
        assert!(device
            .entry(&device.converted)
            .contains(&format!("MSB_HOME={}", device.converted.home.display())));
    }

    /// Moves the user's SSH configuration into a store folder it links to, like a
    /// read-only home-manager file in the Nix store. Returns the folder and a switch
    /// that locks (`true`) or unlocks (`false`) it for this account.
    fn read_only_linked_config(device: &Migrated) -> (PathBuf, impl Fn(bool)) {
        let store = device.directory.path().join("store");
        fs::create_dir_all(&store).unwrap();
        fs::rename(device.user_home.join(".ssh/config"), store.join("config")).unwrap();
        std::os::unix::fs::symlink(store.join("config"), device.user_home.join(".ssh/config"))
            .unwrap();
        let root = unsafe { libc::geteuid() } == 0;
        let folder = store.clone();
        (store, move |locked: bool| {
            if root {
                let owner = if locked { 65534 } else { 0 };
                std::os::unix::fs::chown(&folder, Some(owner), None).unwrap();
                std::os::unix::fs::chown(folder.join("config"), Some(owner), None).unwrap();
            } else {
                fs::set_permissions(
                    &folder,
                    fs::Permissions::from_mode(if locked { 0o555 } else { 0o755 }),
                )
                .unwrap();
            }
        })
    }

    /// A runtime home named `name` holding a copy of the previous one, as a
    /// migration leaves it.
    fn converted_at(device: &Migrated, name: &str) -> RuntimePaths {
        let mut paths = device.converted.clone();
        paths.home = device.user_home.join(".silo").join(name);
        fs::create_dir_all(&paths.home).unwrap();
        let copy = Command::new("/bin/cp")
            .arg("-R")
            .arg(device.previous.home.join("."))
            .arg(&paths.home)
            .status()
            .unwrap();
        assert!(copy.success());
        paths
    }

    impl Migrated {
        fn repair(&self, converted: &RuntimePaths) -> Repair {
            repair_after_migration(converted, &self.user_home, &self.previous.home)
        }

        fn include(&self, home: &str) -> String {
            format!(
                "Include \"{}/.silo/{home}/ssh/*.conf\"",
                self.user_home.display()
            )
        }
    }

    #[test]
    fn a_line_is_reported_only_where_silo_cannot_add_it_and_only_until_it_is_there() {
        let device = migrated();
        let line = device.include(NEW_HOME);
        let needed = |line: &str| manual_include_needed(&device.user_home, Some(line.into()));

        // Silo can write the file: it adds the line itself and has nothing to report.
        let writable = migrated();
        assert_eq!(writable.repair(&writable.converted), Repair::Done);
        assert_eq!(
            manual_include_needed(&writable.user_home, Some(writable.include(NEW_HOME))),
            None
        );

        // Silo can't: every launch reports the line, however often it repeats, and
        // the entries are repointed all the same.
        let (store, lock) = read_only_linked_config(&device);
        let entry = device.converted.home.join("ssh/dev.conf");
        lock(true);
        let launches: Vec<_> = (0..2)
            .map(|_| {
                fs::copy(device.previous.home.join("ssh/dev.conf"), &entry).unwrap();
                (
                    device.repair(&device.converted),
                    device.entry(&device.converted),
                )
            })
            .collect();
        // A different converted home needs a different line.
        let later = converted_at(&device, "cccccccccccc");
        let changed = device.repair(&later);
        lock(false);
        for (repair, entry) in launches {
            assert_eq!(repair, Repair::ManualInclude(line.clone()));
            assert!(entry.contains(&format!("MSB_HOME={}", device.converted.home.display())));
        }
        assert_eq!(
            changed,
            Repair::ManualInclude(device.include("cccccccccccc"))
        );
        assert_eq!(needed(&line), Some(line.clone()));

        // The user adds the line to the file their configuration links to: it is no
        // longer reported, and the next launch finds nothing left to do.
        let old = fs::read(store.join("config")).unwrap();
        fs::write(
            store.join("config"),
            [format!("{line}\n").as_bytes(), &old].concat(),
        )
        .unwrap();
        lock(true);
        let after = device.repair(&device.converted);
        lock(false);
        assert_eq!(after, Repair::Done);
        assert_eq!(needed(&line), None);
        // The line of another home is still needed.
        assert_eq!(
            needed(&device.include("cccccccccccc")),
            Some(device.include("cccccccccccc"))
        );
    }

    #[test]
    fn repair_reports_failed_writes_and_retries_all_transport_directives() {
        // Root can write through directory permission bits.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let device = migrated();
        let root = device.converted.home.join("ssh");
        let entry = root.join("dev.conf");
        let original = fs::read(&entry).unwrap();
        let config = device.config();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o500)).unwrap();
        assert!(write_private(&entry, b"cannot replace").is_err());
        let repair = device.repair(&device.converted);
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(matches!(repair, Repair::Failed(_)), "{repair:?}");
        assert_eq!(fs::read(&entry).unwrap(), original);
        assert_eq!(device.config(), config);
        assert_eq!(device.repair(&device.converted), Repair::Done);
        let updated = device.entry(&device.converted);
        for directive in ["ProxyCommand", "IdentityFile", "UserKnownHostsFile"] {
            let value = updated
                .lines()
                .find(|line| line.starts_with(&format!("  {directive} ")))
                .unwrap();
            assert!(
                value.contains(&device.converted.home.display().to_string()),
                "{value}"
            );
            assert!(!value.contains(&device.previous.home.display().to_string()));
        }
    }

    #[test]
    fn repair_reports_unreadable_files_and_finishes_the_writable_batch() {
        let device = migrated();
        let root = device.converted.home.join("ssh");
        let outside = device.directory.path().join("unrelated-config");
        fs::write(&outside, "keep this file").unwrap();
        let unreadable = root.join("unreadable.conf");
        std::os::unix::fs::symlink(&outside, &unreadable).unwrap();
        let config = device.config();
        let repair = device.repair(&device.converted);
        assert!(
            matches!(&repair, Repair::Failed(error) if error.contains("unreadable.conf")),
            "{repair:?}"
        );
        assert_eq!(fs::read_to_string(&outside).unwrap(), "keep this file");
        assert!(fs::symlink_metadata(&unreadable)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(device.config(), config);
        assert!(device
            .entry(&device.converted)
            .contains(&format!("MSB_HOME={}", device.converted.home.display())));
        fs::remove_file(&unreadable).unwrap();
        assert_eq!(device.repair(&device.converted), Repair::Done);
    }

    #[test]
    fn repair_accepts_an_absent_optional_configuration_directory() {
        let device = migrated();
        fs::remove_dir_all(device.converted.home.join("ssh")).unwrap();
        assert_eq!(device.repair(&device.converted), Repair::Done);
    }

    #[test]
    fn a_failure_other_than_the_line_to_add_is_not_reported_as_one() {
        let device = migrated();
        // A runtime home `ssh` can't be told about: the `Include` can't be written at all.
        let mut broken = device.converted.clone();
        broken.home = device.user_home.join(".silo/bb\nbb");
        assert_eq!(device.repair(&broken), Repair::Failed(FAILED.into()));
    }

    #[test]
    fn the_line_to_add_is_replaced_and_cleared() {
        // The only test that touches the process-wide line.
        assert!(!replace_manual_include(None));
        assert!(replace_manual_include(Some("Include a".into())));
        assert!(!replace_manual_include(Some("Include a".into())));
        assert!(replace_manual_include(Some("Include b".into())));
        assert!(replace_manual_include(None));
        assert!(!replace_manual_include(None));
    }

    #[test]
    fn an_unreadable_or_oversized_config_is_never_rewritten() {
        let device = migrated();
        let config = device.user_home.join(".ssh/config");
        fs::remove_file(&config).unwrap();
        // A dangling link: nothing to read, nothing to replace.
        std::os::unix::fs::symlink("/silo-test-missing/config", &config).unwrap();
        device.repoint().unwrap();
        assert!(fs::symlink_metadata(&config)
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(!config.exists());
    }

    #[test]
    fn only_silo_entries_that_are_regular_files_are_rewritten() {
        let device = migrated();
        let ssh = device.converted.home.join("ssh");
        let foreign = "Host mine\n  HostName example.org\n";
        fs::write(ssh.join("mine.conf"), foreign).unwrap();
        fs::write(ssh.join("bad name.conf"), device.entry(&device.converted)).unwrap();
        let target = device.directory.path().join("target.conf");
        fs::write(&target, device.entry(&device.converted)).unwrap();
        std::os::unix::fs::symlink(&target, ssh.join("linked.conf")).unwrap();
        assert!(device.repoint().unwrap_err().contains("linked.conf"));
        assert_eq!(fs::read_to_string(ssh.join("mine.conf")).unwrap(), foreign);
        assert_eq!(
            fs::read_to_string(ssh.join("bad name.conf")).unwrap(),
            device.entry(&device.previous)
        );
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            device.entry(&device.previous)
        );
    }

    fn app_data_after(generation: Option<&str>, status: Option<&str>) -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        let app_data = directory.path();
        fs::create_dir_all(app_data.join("runtime/microsandbox")).unwrap();
        if let Some(generation) = generation {
            fs::write(
                app_data.join("runtime-generation.json"),
                format!(r#"{{"version":1,"directory":"{generation}"}}"#),
            )
            .unwrap();
            fs::create_dir_all(app_data.join(generation).join("microsandbox")).unwrap();
        }
        if let Some(status) = status {
            fs::write(
                app_data.join("runtime-migration.json"),
                format!(
                    r#"{{"version":1,"status":"{status}","stage":"x","logs":[],"migratedCount":1,"failedCount":0,"totalCount":1,"canContinue":false}}"#
                ),
            )
            .unwrap();
        }
        directory
    }

    #[test]
    fn nothing_is_repointed_unless_the_conversion_completed() {
        let user_home = Path::new("/home/user");
        let after = |generation, status| {
            let directory = app_data_after(generation, status);
            previous_home_after_migration(directory.path(), user_home)
                .map(|home| (home, directory.path().to_path_buf()))
        };
        // Every launch of an install that never migrated, or whose migration has not
        // finished, fails, or went on into a fresh runtime with "Continue".
        assert_eq!(after(None, None), None);
        assert_eq!(after(None, Some("not-required")), None);
        assert_eq!(after(None, Some("scanning")), None);
        assert_eq!(
            after(Some("runtime-checkpoints-clean"), Some("complete")),
            None
        );
        assert_eq!(
            after(Some("runtime-checkpoints-converted"), Some("running")),
            None
        );
        assert_eq!(
            after(Some("runtime-checkpoints-converted"), Some("failed")),
            None
        );
        assert_eq!(after(Some("runtime-checkpoints-converted"), None), None);
        // The converted generation with a complete migration names the previous home.
        let (home, app_data) =
            after(Some("runtime-checkpoints-converted"), Some("complete")).unwrap();
        assert_eq!(
            home,
            runtime::runtime_home_alias(user_home, &app_data.join("runtime/microsandbox"))
        );
        assert_ne!(
            home,
            runtime::runtime_home_alias(
                user_home,
                &app_data.join("runtime-checkpoints-converted/microsandbox")
            )
        );
    }

    #[test]
    #[ignore = "requires an explicitly provided running Silo computer and installs its editor SSH configuration"]
    fn live_editor_transport() {
        crate::test_support::live::require_confirmation();
        let home =
            PathBuf::from(std::env::var("SILO_EDITOR_USER_HOME").expect("explicit user home"));
        let real_home = PathBuf::from(std::env::var_os("HOME").expect("explicit real user home"));
        let home = crate::test_support::live::isolated_editor_home(&home, &real_home).unwrap();
        let runtime_home = PathBuf::from(
            std::env::var("SILO_EDITOR_RUNTIME_HOME").expect("explicit runtime home"),
        );
        let paths = RuntimePaths {
            guest_image: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("runtime/guest-image"),
            executable: PathBuf::from(
                std::env::var("SILO_EDITOR_MSB").expect("explicit bundled runtime"),
            ),
            library: PathBuf::from(
                std::env::var("SILO_EDITOR_LIBRARY").expect("explicit runtime library"),
            ),
            home: runtime_home,
            storage_home: None,
            metadata: PathBuf::new(),
            volumes: PathBuf::new(),
        };
        let (alias, config) = prepare(&paths, &home, "dev").unwrap();
        let mut probe = Command::new("/usr/bin/ssh");
        probe.arg("-F").arg(config).args([
            "-o",
            "ConnectTimeout=5",
            &alias,
            "test -d /workspace/silo-files-test-express/test",
        ]);
        run(&mut probe, Duration::from_secs(10)).unwrap();
        // Reproduce editor-server upload: mkdir relative to the SSH login home,
        // then SCP (SFTP) to that relative path, and read it through SSH again.
        let transfer_directory = format!(".silo-editor-test-{}", uuid::Uuid::new_v4().simple());
        let (_, config) = prepare(&paths, &home, "dev").unwrap();
        let mut mkdir = Command::new("/usr/bin/ssh");
        mkdir.arg("-F").arg(&config).arg(&alias).arg(format!(
            "test \"$(pwd)\" = \"$HOME\" && mkdir {transfer_directory}"
        ));
        run(&mut mkdir, Duration::from_secs(10)).unwrap();
        let mut local = tempfile::NamedTempFile::new().unwrap();
        local.write_all(b"silo-editor-transfer-proof").unwrap();
        let mut copy = Command::new("/usr/bin/scp");
        copy.arg("-F")
            .arg(&config)
            .arg(local.path())
            .arg(format!("{alias}:{transfer_directory}/probe"));
        let copied = run(&mut copy, Duration::from_secs(10));
        let mut verify = Command::new("/usr/bin/ssh");
        verify.arg("-F").arg(&config).arg(&alias).arg(format!(
            "test \"$(cat {transfer_directory}/probe)\" = silo-editor-transfer-proof"
        ));
        let verified = run(&mut verify, Duration::from_secs(10));
        let mut cleanup = Command::new("/usr/bin/ssh");
        cleanup.arg("-F").arg(&config).arg(&alias).arg(format!(
            "rm -f {transfer_directory}/probe && rmdir {transfer_directory}"
        ));
        run(&mut cleanup, Duration::from_secs(10)).unwrap();
        copied.unwrap();
        verified.unwrap();
        if std::env::var_os("SILO_EDITOR_OPEN_ZED").is_some() {
            let uri = remote_uri(&alias, "/workspace/silo-files-test-express", true).unwrap();
            let mut command = Command::new("/Applications/Zed.app/Contents/MacOS/cli");
            command.arg(uri);
            run(&mut command, Duration::from_secs(10)).unwrap();
        }
    }
}
