//! Host access to a provisioned macOS guest: the account `silo`, its per-computer
//! secrets, and commands over the system's `ssh` and `scp`.
//!
//! The secrets live in `<computer dir>/guest-access/` (mode 0700): the account
//! password and an ed25519 key. Both tools run with `-F /dev/null`, so the user's
//! SSH configuration never applies, and with this computer's own `known_hosts`.
use super::store::{self, Layout, Record};
use std::{
    fs,
    io::{Read, Write},
    net::Ipv4Addr,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

pub(super) const USER: &str = "silo";
const LEASES: &str = "/var/db/dhcpd_leases";
const SSH: &str = "/usr/bin/ssh";
const SCP: &str = "/usr/bin/scp";
const KEYGEN: &str = "/usr/bin/ssh-keygen";
const PASSWORD_LENGTH: usize = 24;
const CONNECT_TIMEOUT: u64 = 10;
const SSH_PROBE: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_secs(3);
/// What `run_capped` reports when the command printed more than its allowance.
pub(crate) const OUTPUT_TOO_LARGE: &str = "The command in the computer printed too much.";
pub(super) const CANCELLED: &str = "Setup was cancelled.";

pub(crate) struct CommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

pub(crate) struct GuestAccount {
    pub user: String,
    pub password: String,
    pub key: PathBuf,
}

fn directory(layout: &Layout) -> PathBuf {
    layout.dir.join("guest-access")
}

fn known_hosts(layout: &Layout) -> PathBuf {
    directory(layout).join("known_hosts")
}

/// The account of this computer; its password and key are created the first time.
pub(crate) fn account(layout: &Layout) -> Result<GuestAccount, String> {
    let dir = directory(layout);
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)
        .map_err(|error| store::io_error("create the computer's access folder", &error))?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
        .map_err(|error| store::io_error("protect the computer's access folder", &error))?;

    let password = dir.join("password");
    let password = match fs::read_to_string(&password) {
        Ok(text) if !text.trim().is_empty() => text.trim().to_string(),
        _ => {
            let generated = random_password()?;
            write_secret(&password, generated.as_bytes())?;
            generated
        }
    };

    let key = dir.join("id_ed25519");
    if !key.exists() {
        let output = Command::new(KEYGEN)
            .args(["-q", "-t", "ed25519", "-N", "", "-C", "silo"])
            .arg("-f")
            .arg(&key)
            .output()
            .map_err(|error| format!("Silo could not create the computer's SSH key: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "Silo could not create the computer's SSH key: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
    }
    if !public_key_path(&key).exists() {
        let output = Command::new(KEYGEN)
            .arg("-y")
            .arg("-f")
            .arg(&key)
            .output()
            .map_err(|error| format!("Silo could not read the computer's SSH key: {error}"))?;
        if !output.status.success() {
            return Err("Silo could not read the computer's SSH key.".into());
        }
        fs::write(public_key_path(&key), &output.stdout)
            .map_err(|error| store::io_error("save the computer's SSH key", &error))?;
    }
    Ok(GuestAccount {
        user: USER.into(),
        password,
        key,
    })
}

pub(super) fn public_key_path(key: &Path) -> PathBuf {
    let mut name = key.as_os_str().to_os_string();
    name.push(".pub");
    PathBuf::from(name)
}

pub(super) fn public_key(account: &GuestAccount) -> Result<String, String> {
    fs::read_to_string(public_key_path(&account.key))
        .map(|text| text.trim().to_string())
        .map_err(|error| store::io_error("read the computer's SSH key", &error))
}

fn write_secret(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|error| store::io_error("save the computer's password", &error))
}

fn random_password() -> Result<String, String> {
    const ALPHABET: &[u8] = b"abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut bytes = [0u8; PASSWORD_LENGTH];
    fs::File::open("/dev/urandom")
        .and_then(|mut random| random.read_exact(&mut bytes))
        .map_err(|error| store::io_error("create a password", &error))?;
    // 256 is not a multiple of the alphabet size; the bias is irrelevant for a
    // 24-character password that only ever protects a local guest.
    Ok(bytes
        .iter()
        .map(|byte| ALPHABET[usize::from(*byte) % ALPHABET.len()] as char)
        .collect())
}

// MARK: Address

struct Lease {
    mac: String,
    ip: Ipv4Addr,
    expires: u64,
}

/// `1,a:b:c` or `a:b:c` as two-digit lowercase groups, so `66:0:f6` matches `66:00:f6`.
fn normalize_mac(mac: &str) -> String {
    mac.split(':')
        .map(|part| {
            let part = part.trim().to_ascii_lowercase();
            if part.len() == 1 {
                format!("0{part}")
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join(":")
}

/// The leases of a vmnet `dhcpd_leases` file that carry an Ethernet address and a valid IPv4 address.
fn parse_leases(text: &str) -> Vec<Lease> {
    let mut leases = Vec::new();
    let mut block: Option<(Option<String>, Option<Ipv4Addr>, Option<u64>)> = None;
    for line in text.lines().map(str::trim) {
        if line == "{" {
            block = Some((None, None, None));
        } else if line == "}" {
            if let Some((Some(mac), Some(ip), Some(expires))) = block.take() {
                leases.push(Lease { mac, ip, expires });
            }
            block = None;
        } else if let (Some(block), Some((key, value))) = (block.as_mut(), line.split_once('=')) {
            let value = value.trim();
            match key.trim() {
                "hw_address" => {
                    block.0 = value
                        .split_once(',')
                        .map(|(_, mac)| normalize_mac(mac.trim()))
                }
                "ip_address" => block.1 = value.parse().ok(),
                "lease" => {
                    block.2 = u64::from_str_radix(value.trim_start_matches("0x"), 16).ok();
                }
                _ => {}
            }
        }
    }
    leases
}

/// The address of the newest lease held by `mac_address`.
fn newest_address(text: &str, mac_address: &str) -> Option<Ipv4Addr> {
    let wanted = normalize_mac(mac_address);
    parse_leases(text)
        .into_iter()
        .filter(|lease| lease.mac == wanted)
        .max_by_key(|lease| lease.expires)
        .map(|lease| lease.ip)
}

/// The address the guest with `mac_address` was last leased by the host's NAT.
pub(crate) fn guest_address(mac_address: &str) -> Result<Ipv4Addr, String> {
    let text = fs::read_to_string(LEASES)
        .map_err(|_| "Silo could not read the Mac's network leases.".to_string())?;
    newest_address(&text, mac_address)
        .ok_or_else(|| "The computer has no network address yet.".to_string())
}

// MARK: Commands

fn quote_option(path: &Path) -> String {
    format!("\"{}\"", path.to_string_lossy().replace('"', "\\\""))
}

/// How a connection proves who it is. Only the first connection to a new guest
/// uses the password; everything after it uses the key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Auth {
    Key,
    Password,
}

fn askpass(layout: &Layout) -> PathBuf {
    directory(layout).join("askpass")
}

/// Writes the helper `ssh` runs to ask for the password. It prints the password
/// file, so the password never appears in an argument or the environment.
fn ensure_askpass(layout: &Layout) -> Result<PathBuf, String> {
    let path = askpass(layout);
    let script = format!(
        "#!/bin/sh\nexec /bin/cat {}\n",
        shell_quote(&directory(layout).join("password").to_string_lossy())
    );
    if fs::read_to_string(&path).is_ok_and(|current| current == script) {
        return Ok(path);
    }
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o700)
        .open(&path)
        .and_then(|mut file| file.write_all(script.as_bytes()))
        .map_err(|error| store::io_error("save the computer's password helper", &error))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
        .map_err(|error| store::io_error("protect the computer's password helper", &error))?;
    Ok(path)
}

/// Environment variables a connection needs.
fn environment(layout: &Layout, auth: Auth) -> Result<Vec<(String, String)>, String> {
    match auth {
        Auth::Key => Ok(Vec::new()),
        Auth::Password => Ok(vec![
            (
                "SSH_ASKPASS".into(),
                ensure_askpass(layout)?.to_string_lossy().into_owned(),
            ),
            ("SSH_ASKPASS_REQUIRE".into(), "force".into()),
        ]),
    }
}

/// Options shared by `ssh` and `scp`.
fn tool_options(
    layout: &Layout,
    record: &Record,
    account: &GuestAccount,
    auth: Auth,
) -> Vec<String> {
    let option = |text: String| ["-o".to_string(), text];
    let mut args = vec!["-F".to_string(), "/dev/null".into()];
    let mut options = match auth {
        Auth::Key => {
            args.extend(["-i".to_string(), account.key.to_string_lossy().into_owned()]);
            vec![
                "IdentitiesOnly=yes".to_string(),
                "BatchMode=yes".into(),
                "PasswordAuthentication=no".into(),
            ]
        }
        Auth::Password => vec![
            "PubkeyAuthentication=no".to_string(),
            "PreferredAuthentications=password,keyboard-interactive".into(),
            "NumberOfPasswordPrompts=1".into(),
        ],
    };
    options.extend([
        "StrictHostKeyChecking=accept-new".to_string(),
        format!("UserKnownHostsFile={}", quote_option(&known_hosts(layout))),
        format!("HostKeyAlias=silo-{}", record.id),
        format!("ConnectTimeout={CONNECT_TIMEOUT}"),
        "ServerAliveInterval=15".into(),
        "LogLevel=ERROR".into(),
    ]);
    for text in options {
        args.extend(option(text));
    }
    args
}

fn ssh_args(
    layout: &Layout,
    record: &Record,
    account: &GuestAccount,
    address: Ipv4Addr,
    command: &str,
    auth: Auth,
) -> Vec<String> {
    let mut args = tool_options(layout, record, account, auth);
    args.push(format!("{}@{address}", account.user));
    args.push(command.into());
    args
}

fn scp_args(
    layout: &Layout,
    record: &Record,
    account: &GuestAccount,
    address: Ipv4Addr,
    local: &Path,
    remote: &str,
) -> Vec<String> {
    let mut args = vec!["-q".to_string()];
    args.extend(tool_options(layout, record, account, Auth::Key));
    args.push(local.to_string_lossy().into_owned());
    args.push(format!("{}@{address}:{remote}", account.user));
    args
}

/// Waits until the guest answers a key login over SSH and returns its address.
pub(crate) fn wait_for_ssh(
    layout: &Layout,
    record: &Record,
    timeout: Duration,
    cancel: &dyn Fn() -> bool,
) -> Result<Ipv4Addr, String> {
    wait(layout, record, timeout, cancel, Auth::Key)
}

/// Waits until the guest answers a password login. Only a guest whose key is not
/// installed yet needs this.
pub(super) fn wait_for_password_ssh(
    layout: &Layout,
    record: &Record,
    timeout: Duration,
    cancel: &dyn Fn() -> bool,
) -> Result<Ipv4Addr, String> {
    wait(layout, record, timeout, cancel, Auth::Password)
}

fn wait(
    layout: &Layout,
    record: &Record,
    timeout: Duration,
    cancel: &dyn Fn() -> bool,
    auth: Auth,
) -> Result<Ipv4Addr, String> {
    let account = account(layout)?;
    let envs = environment(layout, auth)?;
    let deadline = Instant::now() + timeout;
    loop {
        if cancel() {
            return Err(CANCELLED.into());
        }
        if let Ok(address) = guest_address(&record.mac_address) {
            let args = ssh_args(layout, record, &account, address, "true", auth);
            if exec(SSH, &args, None, SSH_PROBE, &envs, None).is_ok_and(|output| output.status == 0)
            {
                return Ok(address);
            }
        }
        if Instant::now() >= deadline {
            return Err("The computer did not accept SSH logins in time.".into());
        }
        thread::sleep(POLL);
    }
}

/// Runs `command` as `silo` in the guest. Root work uses `sudo -n` inside the command.
pub(crate) fn run(
    layout: &Layout,
    record: &Record,
    command: &str,
    stdin: Option<&[u8]>,
    timeout: Duration,
) -> Result<CommandOutput, String> {
    run_as(layout, record, command, stdin, timeout, Auth::Key, None)
}

/// Like `run`, but fails with `OUTPUT_TOO_LARGE` and stops the command once it has printed more
/// than `max_stdout` bytes, so the host never buffers more than that.
pub(crate) fn run_capped(
    layout: &Layout,
    record: &Record,
    command: &str,
    stdin: Option<&[u8]>,
    timeout: Duration,
    max_stdout: Option<usize>,
) -> Result<CommandOutput, String> {
    run_as(
        layout,
        record,
        command,
        stdin,
        timeout,
        Auth::Key,
        max_stdout,
    )
}

/// Like `run`, logging in with the password.
pub(super) fn run_with_password(
    layout: &Layout,
    record: &Record,
    command: &str,
    stdin: Option<&[u8]>,
    timeout: Duration,
) -> Result<CommandOutput, String> {
    run_as(
        layout,
        record,
        command,
        stdin,
        timeout,
        Auth::Password,
        None,
    )
}

fn run_as(
    layout: &Layout,
    record: &Record,
    command: &str,
    stdin: Option<&[u8]>,
    timeout: Duration,
    auth: Auth,
    max_stdout: Option<usize>,
) -> Result<CommandOutput, String> {
    let account = account(layout)?;
    let address = guest_address(&record.mac_address)?;
    exec(
        SSH,
        &ssh_args(layout, record, &account, address, command, auth),
        stdin,
        timeout,
        &environment(layout, auth)?,
        max_stdout,
    )
}

/// Copies `local` to `remote` in the guest.
// Used by the installers that copy files into the guest.
#[allow(dead_code)]
pub(crate) fn copy(
    layout: &Layout,
    record: &Record,
    local: &Path,
    remote: &str,
    timeout: Duration,
) -> Result<(), String> {
    let account = account(layout)?;
    let address = guest_address(&record.mac_address)?;
    let output = exec(
        SCP,
        &scp_args(layout, record, &account, address, local, remote),
        None,
        timeout,
        &[],
        None,
    )?;
    if output.status == 0 {
        Ok(())
    } else {
        Err(format!(
            "Silo could not copy a file to the computer: {}",
            output.stderr.trim()
        ))
    }
}

/// Runs a host tool to completion, killing it when `timeout` passes.
fn exec(
    program: &str,
    args: &[String],
    stdin: Option<&[u8]>,
    timeout: Duration,
    envs: &[(String, String)],
    max_stdout: Option<usize>,
) -> Result<CommandOutput, String> {
    let mut child = Command::new(program)
        .args(args)
        .envs(envs.iter().map(|(key, value)| (key, value)))
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Silo could not run {program}: {error}"))?;
    let writer = stdin.map(|bytes| {
        let (mut pipe, bytes) = (child.stdin.take(), bytes.to_vec());
        thread::spawn(move || {
            if let Some(pipe) = pipe.as_mut() {
                let _ = pipe.write_all(&bytes);
            }
        })
    });
    let exceeded = Arc::new(AtomicBool::new(false));
    let stdout = drain(child.stdout.take(), max_stdout, exceeded.clone());
    let stderr = drain(child.stderr.take(), None, Arc::new(AtomicBool::new(false)));
    let deadline = Instant::now() + timeout;
    let status = loop {
        if exceeded.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(OUTPUT_TOO_LARGE.into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("The command in the computer took too long.".into());
            }
            Err(error) => return Err(format!("Silo lost the command in the computer: {error}")),
        }
    };
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    Ok(CommandOutput {
        status: status.code().unwrap_or(-1),
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

/// Reads `stream` to its end. Past `limit` bytes it stops reading, sets `exceeded` and returns
/// what it has.
fn drain<R: Read + Send + 'static>(
    stream: Option<R>,
    limit: Option<usize>,
    exceeded: Arc<AtomicBool>,
) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut text = Vec::new();
        if let Some(stream) = stream {
            match limit {
                None => {
                    let mut stream = stream;
                    let _ = stream.read_to_end(&mut text);
                }
                Some(limit) => {
                    let _ = stream.take(limit as u64 + 1).read_to_end(&mut text);
                    if text.len() > limit {
                        exceeded.store(true, Ordering::SeqCst);
                    }
                }
            }
        }
        String::from_utf8_lossy(&text).into_owned()
    })
}

/// A single-quoted shell word.
pub(super) fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEASES_TEXT: &str = "{
\tname=old
\tip_address=192.168.64.4
\thw_address=1,66:0:f6:3f:92:2d
\tidentifier=1,66:0:f6:3f:92:2d
\tlease=0x6ab424ac
}
{
\tname=silo
\tip_address=192.168.64.9
\thw_address=1,66:0:f6:3f:92:2d
\tidentifier=1,66:0:f6:3f:92:2d
\tlease=0x6ac65cc1
}
{
\tname=other
\tip_address=192.168.64.5
\thw_address=1,8a:c4:f4:70:e1:96
\tlease=0x6aba833e
}
{
\tname=infiniband
\tip_address=192.168.64.6
\thw_address=ff,f1:f5:dd:7f:0:2:0:0:ab:11:b5:26:c6:38:da:c:30:60
\tlease=0x6ab424ac
}
{
\tname=broken
\tip_address=not-an-address
\thw_address=1,02:00:00:00:00:01
\tlease=0x6ab424ac
}
";

    #[test]
    fn the_newest_lease_of_a_mac_wins() {
        assert_eq!(
            newest_address(LEASES_TEXT, "66:00:f6:3f:92:2d"),
            Some(Ipv4Addr::new(192, 168, 64, 9))
        );
    }

    #[test]
    fn leading_zeros_and_case_do_not_matter() {
        assert_eq!(
            newest_address(LEASES_TEXT, "66:0:F6:3F:92:2D"),
            Some(Ipv4Addr::new(192, 168, 64, 9))
        );
        assert_eq!(
            newest_address(LEASES_TEXT, "8A:C4:F4:70:E1:96"),
            Some(Ipv4Addr::new(192, 168, 64, 5))
        );
    }

    #[test]
    fn unknown_macs_and_unusable_leases_have_no_address() {
        assert_eq!(newest_address(LEASES_TEXT, "02:00:00:00:00:99"), None);
        assert_eq!(newest_address(LEASES_TEXT, "02:00:00:00:00:01"), None);
        assert_eq!(newest_address("", "66:00:f6:3f:92:2d"), None);
    }

    fn fixture() -> (tempfile::TempDir, Layout, Record, GuestAccount) {
        let dir = tempfile::tempdir().unwrap();
        let request = store::CreateRequest {
            name: "mac-one".into(),
            cpus: 4,
            memory_gib: 8,
            disk_gib: 64,
        };
        let record = store::new_record(&request, "02:00:00:00:00:01".into());
        let layout = Layout::new(dir.path(), &record.id);
        let account = GuestAccount {
            user: USER.into(),
            password: "pw".into(),
            key: layout.dir.join("guest-access/id_ed25519"),
        };
        (dir, layout, record, account)
    }

    #[test]
    fn ssh_ignores_the_users_configuration_and_keys() {
        let (_dir, layout, record, account) = fixture();
        let args = ssh_args(
            &layout,
            &record,
            &account,
            Ipv4Addr::new(192, 168, 64, 9),
            "sudo -n true",
            Auth::Key,
        );
        assert_eq!(&args[..2], ["-F", "/dev/null"]);
        for option in [
            "IdentitiesOnly=yes",
            "BatchMode=yes",
            "PasswordAuthentication=no",
            "StrictHostKeyChecking=accept-new",
        ] {
            assert!(args.iter().any(|arg| arg == option), "{option}");
        }
        assert!(args.contains(&format!("HostKeyAlias=silo-{}", record.id)));
        assert!(args
            .iter()
            .any(|arg| arg.starts_with("UserKnownHostsFile=\"")
                && arg.ends_with("guest-access/known_hosts\"")));
        assert_eq!(args[args.len() - 2], "silo@192.168.64.9");
        assert_eq!(args[args.len() - 1], "sudo -n true");
    }

    #[test]
    fn the_first_connection_logs_in_with_the_password_through_an_askpass_helper() {
        let (_dir, layout, record, account) = fixture();
        let account_files = super::account(&layout).unwrap();
        let args = ssh_args(
            &layout,
            &record,
            &account,
            Ipv4Addr::new(192, 168, 64, 9),
            "true",
            Auth::Password,
        );
        assert!(args.contains(&"PubkeyAuthentication=no".to_string()));
        assert!(args.contains(&"NumberOfPasswordPrompts=1".to_string()));
        assert!(!args.iter().any(|arg| arg == "BatchMode=yes" || arg == "-i"));
        // The password itself is never an argument.
        assert!(!args.iter().any(|arg| arg.contains(&account_files.password)));

        let envs = environment(&layout, Auth::Password).unwrap();
        assert!(envs.contains(&("SSH_ASKPASS_REQUIRE".to_string(), "force".to_string())));
        let helper = envs.iter().find(|(key, _)| key == "SSH_ASKPASS").unwrap();
        assert!(envs
            .iter()
            .all(|(_, value)| !value.contains(&account_files.password)));
        let path = PathBuf::from(&helper.1);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let output = exec(
            path.to_str().unwrap(),
            &["Password:".to_string()],
            None,
            Duration::from_secs(5),
            &[],
            None,
        )
        .unwrap();
        assert_eq!(output.stdout, account_files.password);
        assert!(environment(&layout, Auth::Key).unwrap().is_empty());
    }

    #[test]
    fn scp_copies_to_the_account_in_the_guest() {
        let (_dir, layout, record, account) = fixture();
        let args = scp_args(
            &layout,
            &record,
            &account,
            Ipv4Addr::new(192, 168, 64, 9),
            Path::new("/tmp/a b"),
            "/tmp/b",
        );
        assert_eq!(args[0], "-q");
        assert_eq!(args[args.len() - 2], "/tmp/a b");
        assert_eq!(args[args.len() - 1], "silo@192.168.64.9:/tmp/b");
    }

    #[test]
    fn secrets_are_created_once_and_protected() {
        let (_dir, layout, _record, _account) = fixture();
        let first = account(&layout).unwrap();
        let second = account(&layout).unwrap();
        assert_eq!(first.user, "silo");
        assert_eq!(first.password, second.password);
        assert_eq!(first.password.len(), PASSWORD_LENGTH);
        assert_eq!(first.key, second.key);
        assert!(public_key(&first).unwrap().starts_with("ssh-ed25519 "));
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&directory(&layout)), 0o700);
        assert_eq!(mode(&directory(&layout).join("password")), 0o600);
        assert_eq!(mode(&first.key), 0o600);
    }

    #[test]
    fn a_lost_public_key_is_derived_again() {
        let (_dir, layout, _record, _account) = fixture();
        let first = account(&layout).unwrap();
        let before = public_key(&first).unwrap();
        fs::remove_file(public_key_path(&first.key)).unwrap();
        let again = account(&layout).unwrap();
        assert_eq!(
            public_key(&again).unwrap().split(' ').nth(1),
            before.split(' ').nth(1)
        );
    }

    #[test]
    fn commands_report_output_status_and_timeouts() {
        let args = |script: &str| vec!["-c".to_string(), script.to_string()];
        let output = exec(
            "/bin/sh",
            &args("cat; echo err >&2; exit 3"),
            Some(b"in"),
            Duration::from_secs(5),
            &[("SILO_TEST".to_string(), "env".to_string())],
            None,
        )
        .unwrap();
        assert_eq!(output.status, 3);
        assert_eq!(output.stdout, "in");
        assert_eq!(output.stderr.trim(), "err");
        assert!(exec(
            "/bin/sh",
            &args("sleep 5"),
            None,
            Duration::from_millis(200),
            &[],
            None,
        )
        .is_err());
    }

    #[test]
    fn output_beyond_the_allowance_stops_the_command() {
        let args = |script: &str| vec!["-c".to_string(), script.to_string()];
        let within = exec(
            "/bin/sh",
            &args("printf 12345"),
            None,
            Duration::from_secs(5),
            &[],
            Some(5),
        )
        .unwrap();
        assert_eq!(within.stdout, "12345");
        let started = Instant::now();
        let error = exec(
            "/bin/sh",
            &args("printf 123456; sleep 30"),
            None,
            Duration::from_secs(20),
            &[],
            Some(5),
        )
        .err()
        .unwrap();
        assert_eq!(error, OUTPUT_TOO_LARGE);
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn shell_words_are_quoted() {
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }
}
