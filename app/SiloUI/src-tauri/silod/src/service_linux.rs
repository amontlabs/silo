//! `silod --service` on Linux: a systemd user unit in place of the macOS LaunchAgent.
//!
//! `register` writes `<channel>.engine.service` (`org.silo.dev.engine.service` for Dev,
//! `org.silo.preview.engine.service` for production) to the user's systemd unit directory,
//! reloads the user manager, and enables and starts the unit. `unregister` disables and stops
//! it and removes the file. Each command then prints the status: the same first line as on
//! macOS (`enabled` or `notRegistered`, and `disabled` for a unit file that is not enabled),
//! then the unit's state, its path, whether it starts this `silod`, and whether the account
//! lingers.
//!
//! The unit mirrors the LaunchAgent: it runs `silod --agent`, starts with the user manager,
//! restarts after a failed exit but not after a clean one (exit code 75, `RESTART_EXIT_CODE`,
//! is a failure to systemd, so a requested restart starts a new engine), sends SIGTERM to
//! `silod` alone with 30 seconds to end its session, and runs with umask 077.
//!
//! The engine's computers run in their own processes. The engine stops them in its session
//! end; one it could not stop in time stays in the unit's cgroup after silod exits, the engine
//! exits with `SESSION_END_INCOMPLETE_EXIT_CODE`, and `unregister` names what survived.
//!
//! Without lingering, the user manager, and so the engine, starts at the first login and stops
//! at the last logout, and the user manager ending then kills whatever still runs under it,
//! computers included. Turning lingering on needs privileges (`loginctl enable-linger`), so
//! silod only reports it and says how.

use std::{
    ffi::CStr,
    fs,
    io::{self, Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
};

use silo_engine::channel::Channel;

/// The first line of every unit file silod writes. A file at the unit's path without it is
/// someone else's, and silod never overwrites or removes it.
const MARKER: &str =
    "# Written by `silod --service register`. silod rewrites and removes this file.";

/// The service's label, the same as the LaunchAgent's (`<channel>.engine`).
pub fn engine_service_name(channel: Channel) -> String {
    format!("{}.engine", channel.identifier())
}

/// The unit's file name, from the channel like the LaunchAgent plist's.
pub fn unit_name(channel: Channel) -> String {
    format!("{}.service", engine_service_name(channel))
}

/// The output of one `systemctl` or `loginctl` call.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Runs the system's service tools. Tests replace it; the real one runs the program by name.
pub trait ServiceTools {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, String>;
}

struct SystemTools;

impl ServiceTools for SystemTools {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, String> {
        let output = Command::new(program)
            .args(args)
            .output()
            .map_err(|error| format!("Could not run {program}: {error}"))?;
        Ok(CommandOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

/// Everything a service command needs to know about this engine and account.
#[derive(Clone, Debug)]
pub struct ServiceSetup {
    pub channel: Channel,
    /// The canonical path of this `silod`, which the unit starts.
    pub executable: PathBuf,
    /// `$XDG_CONFIG_HOME/systemd/user`, by default `~/.config/systemd/user`.
    pub unit_dir: PathBuf,
    /// The account's UID, which `loginctl` is asked about.
    pub uid: u32,
    /// The account's name from the user database for the lingering advice (its UID when the
    /// database has none).
    pub user: String,
    /// Where the cgroup hierarchy is mounted (`/sys/fs/cgroup`).
    pub cgroup_root: PathBuf,
    /// Where `/proc` is mounted.
    pub proc_root: PathBuf,
}

impl ServiceSetup {
    pub fn unit_path(&self) -> PathBuf {
        self.unit_dir.join(unit_name(self.channel))
    }
}

pub fn run_command(args: &[String]) -> Result<i32, String> {
    let Some(command) = args.first().map(String::as_str) else {
        return Err("usage: silod --service status|register|unregister".to_owned());
    };
    if args.len() != 1 {
        return Err("--service accepts exactly one command".to_owned());
    }
    match command {
        "status" | "register" | "unregister" => {}
        "open-settings" => {
            return Err(
                "silod --service open-settings is macOS only; on Linux, use systemctl --user"
                    .to_owned(),
            )
        }
        _ => return Err(format!("unsupported service command: {command}")),
    }
    let manifest = crate::app::trusted_manifest()?;
    let channel = manifest.channel()?;
    let executable = std::env::current_exe()
        .and_then(fs::canonicalize)
        .map_err(|error| format!("Cannot locate silod: {error}"))?;
    let uid = unsafe { libc::getuid() };
    let setup = ServiceSetup {
        channel,
        executable,
        unit_dir: user_unit_dir(|name| std::env::var_os(name))?,
        uid,
        user: account_name(uid),
        cgroup_root: PathBuf::from("/sys/fs/cgroup"),
        proc_root: PathBuf::from("/proc"),
    };
    run_with(
        command,
        &setup,
        &SystemTools,
        &mut io::stdout(),
        &mut io::stderr(),
    )
}

/// `$XDG_CONFIG_HOME/systemd/user`, or `$HOME/.config/systemd/user` when the variable is unset,
/// empty or relative (the XDG specification ignores those), where the user manager looks.
pub fn user_unit_dir(
    variable: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Result<PathBuf, String> {
    let config = match variable("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    {
        Some(path) => path,
        None => variable("HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or_else(|| "The user's home directory is unavailable".to_owned())?
            .join(".config"),
    };
    Ok(config.join("systemd/user"))
}

/// The name the user database (`getpwuid_r`) gives `uid`, or the UID itself. The environment
/// (`$USER`) is never consulted: `su` and `sudo` keep or change it.
pub fn account_name(uid: u32) -> String {
    let mut buffer = vec![0 as libc::c_char; 1024];
    loop {
        let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        let code = unsafe {
            libc::getpwuid_r(
                uid,
                &mut entry,
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut result,
            )
        };
        if code == libc::ERANGE && buffer.len() < 1 << 20 {
            let larger = buffer.len() * 2;
            buffer.resize(larger, 0);
            continue;
        }
        if code != 0 || result.is_null() || entry.pw_name.is_null() {
            return uid.to_string();
        }
        // The entry's strings live in `buffer`, which outlives this borrow.
        let name = unsafe { CStr::from_ptr(entry.pw_name) };
        return name
            .to_str()
            .ok()
            .filter(|name| {
                !name.is_empty() && !name.chars().any(|c| c.is_whitespace() || c.is_control())
            })
            .map_or_else(|| uid.to_string(), str::to_owned);
    }
}

/// Runs one service command against `tools`, writing the status to `out` and advice to `err`.
pub fn run_with(
    command: &str,
    setup: &ServiceSetup,
    tools: &dyn ServiceTools,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32, String> {
    match command {
        "status" => {}
        "register" => register(setup, tools, err)?,
        "unregister" => unregister(setup, tools, err)?,
        _ => return Err(format!("unsupported service command: {command}")),
    }
    report(setup, tools, out, err)?;
    Ok(0)
}

/// The unit file `register` writes for this engine.
pub fn unit_file(channel: Channel, executable: &Path) -> Result<String, String> {
    let program = exec_argument(executable)?;
    Ok(format!(
        "{MARKER}
[Unit]
Description={product} engine (silod)
Documentation=https://github.com/amontlabs/silo/blob/main/docs/SiloUI-ENGINE.md#silod-on-a-linux-host
# A failed engine is restarted however often it fails, never left stopped by a rate limit.
StartLimitIntervalSec=0

[Service]
Type=simple
ExecStart={program} --agent
# Exit code 75 asks for a new engine; a clean exit stays stopped.
Restart=on-failure
RestartSec=10
UMask=0077
# SIGTERM starts the engine's bounded session end, which stops its computers itself.
# Only silod gets it: computers it leaves running on purpose (an update) keep running.
KillMode=process
KillSignal=SIGTERM
TimeoutStopSec=30

[Install]
WantedBy=default.target
",
        product = channel.product_name(),
    ))
}

/// `path` as one ExecStart word: quoted, with what systemd would expand escaped.
fn exec_argument(path: &Path) -> Result<String, String> {
    let text = path
        .to_str()
        .ok_or_else(|| format!("silod's path is not UTF-8: {}", path.display()))?;
    if !path.is_absolute() || text.chars().any(char::is_control) {
        return Err(format!(
            "silod's path cannot start a systemd unit: {}",
            path.display()
        ));
    }
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for character in text.chars() {
        match character {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '%' => quoted.push_str("%%"),
            '$' => quoted.push_str("$$"),
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    Ok(quoted)
}

/// What is at the unit's path.
#[derive(Debug, PartialEq, Eq)]
enum UnitFile {
    Missing,
    /// silod's, with its contents.
    Silo(String),
}

/// The largest unit file silod reads; its own are far smaller.
const UNIT_LIMIT: u64 = 64 * 1024;

fn read_unit(path: &Path) -> Result<UnitFile, String> {
    let foreign = || {
        format!(
            "{} exists and was not written by silod; move it away to register the Silo engine",
            path.display()
        )
    };
    match read_marked(path) {
        Ok(Some(contents)) => Ok(UnitFile::Silo(contents)),
        Ok(None) => Err(foreign()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(UnitFile::Missing),
        Err(error) => Err(format!("Cannot read {}: {error}", path.display())),
    }
}

/// The contents of the regular file at `path` when its first line is silod's marker, `None`
/// for anything else there (a link, a directory, a FIFO, a large or foreign file). The file is
/// opened without following a link and checked through the open handle, so what is read is
/// what was checked.
fn read_marked(path: &Path) -> io::Result<Option<String>> {
    let file = match fs::OpenOptions::new()
        .read(true)
        // A FIFO must not block the open; a link is refused (ELOOP).
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.raw_os_error() == Some(libc::ELOOP) => return Ok(None),
        Err(error) => return Err(error),
    };
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file() || metadata.len() > UNIT_LIMIT {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    file.take(UNIT_LIMIT + 1).read_to_end(&mut bytes)?;
    let Ok(contents) = String::from_utf8(bytes) else {
        return Ok(None);
    };
    if contents.len() as u64 > UNIT_LIMIT || contents.lines().next() != Some(MARKER) {
        return Ok(None);
    }
    Ok(Some(contents))
}

/// Writes `contents` to `path` through a new file in the same directory and a rename.
fn write_unit(path: &Path, contents: &str) -> Result<(), String> {
    let directory = path.parent().expect("the unit path has a directory");
    fs::create_dir_all(directory)
        .map_err(|error| format!("Cannot create {}: {error}", directory.display()))?;
    let temporary = directory.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .expect("the unit path has a file name")
            .to_string_lossy(),
        std::process::id()
    ));
    let create = || {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temporary)
    };
    let written = (|| -> io::Result<()> {
        let mut file = match create() {
            // A run that crashed between create and rename under a PID now reused left this
            // file; it is silod's when it carries the marker, and only then is it replaced.
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                if read_marked(&temporary)?.is_none() {
                    return Err(error);
                }
                fs::remove_file(&temporary)?;
                create()?
            }
            other => other?,
        };
        file.write_all(contents.as_bytes())?;
        // The engine's umask is 077; the user manager only needs to read the file.
        file.set_permissions(fs::Permissions::from_mode(0o644))?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if let Err(error) = written {
        if error.kind() != io::ErrorKind::AlreadyExists {
            let _ = fs::remove_file(&temporary);
        }
        return Err(format!("Cannot write {}: {error}", path.display()));
    }
    Ok(())
}

fn systemctl(tools: &dyn ServiceTools, args: &[&str]) -> Result<CommandOutput, String> {
    let mut full = vec!["--user"];
    full.extend_from_slice(args);
    tools.run("systemctl", &full)
}

/// Runs `systemctl --user <args>` and fails with its message when it fails.
fn systemctl_checked(tools: &dyn ServiceTools, args: &[&str]) -> Result<(), String> {
    let output = systemctl(tools, args)?;
    if output.success {
        return Ok(());
    }
    let message = output.stderr.trim();
    let mut error = format!("systemctl --user {} failed", args.join(" "));
    if !message.is_empty() {
        error.push_str(": ");
        error.push_str(message);
    }
    if message.contains("bus") || message.contains("XDG_RUNTIME_DIR") {
        error.push_str(
            ". This shell cannot reach the user's systemd manager: run it from a login session \
             (not su or sudo), or set XDG_RUNTIME_DIR=/run/user/$(id -u)",
        );
    }
    Err(error)
}

fn register(
    setup: &ServiceSetup,
    tools: &dyn ServiceTools,
    err: &mut dyn Write,
) -> Result<(), String> {
    let path = setup.unit_path();
    let wanted = unit_file(setup.channel, &setup.executable)?;
    let previous = read_unit(&path)?;
    let changed = previous != UnitFile::Silo(wanted.clone());
    let was_active =
        matches!(previous, UnitFile::Silo(_)) && active_state(setup, tools)? == "active";
    if changed {
        write_unit(&path, &wanted)?;
    }
    let unit = unit_name(setup.channel);
    systemctl_checked(tools, &["daemon-reload"])?;
    systemctl_checked(tools, &["enable", "--now", &unit])?;
    if changed && was_active {
        // enable --now leaves a running engine as it is. Restarting it here would end its
        // session and stop its computers, so that is left to the owner.
        let _ = writeln!(
            err,
            "The engine is still running from the previous unit; it starts from the new one \
             after: systemctl --user restart {unit}"
        );
    }
    Ok(())
}

fn unregister(
    setup: &ServiceSetup,
    tools: &dyn ServiceTools,
    err: &mut dyn Write,
) -> Result<(), String> {
    let path = setup.unit_path();
    if read_unit(&path)? == UnitFile::Missing {
        return Ok(());
    }
    let unit = unit_name(setup.channel);
    // Asked before the stop: systemd may forget the path once the unit is inactive, but the
    // cgroup itself stays while anything is left in it. For a unit already inactive, the path
    // is rebuilt from its slice's.
    let cgroup = control_group(tools, &unit);
    // Stopping waits for the engine's session end, which stops its computers.
    systemctl_checked(tools, &["disable", "--now", &unit])?;
    if let Some(cgroup) = cgroup {
        let survivors = remaining_processes(setup, &cgroup);
        if !survivors.is_empty() {
            let names: Vec<String> = survivors
                .iter()
                .map(|(pid, name)| format!("{name} (PID {pid})"))
                .collect();
            let _ = writeln!(
                err,
                "The engine stopped, but its session end left these processes of its computers \
                 running: {}. They keep running without the engine. Starting the engine again \
                 (silod --service register, or silod --agent) takes them back, and it can stop \
                 them then.",
                names.join(", ")
            );
        }
    }
    // A session end that left computers running exits with a failure; the unit going away
    // should not stay listed as failed.
    let _ = systemctl(tools, &["reset-failed", &unit]);
    fs::remove_file(&path).map_err(|error| format!("Cannot remove {}: {error}", path.display()))?;
    systemctl_checked(tools, &["daemon-reload"])
}

/// The unit's cgroup (`/user.slice/.../<unit>`): the one systemd names, or, for an inactive
/// unit it names none for, its slice's cgroup joined with the unit's name.
fn control_group(tools: &dyn ServiceTools, unit: &str) -> Option<String> {
    if let Some(path) = show_value(tools, unit, "ControlGroup").filter(|path| valid_cgroup(path)) {
        return Some(path);
    }
    let slice = show_value(tools, unit, "Slice")?;
    if !slice.ends_with(".slice") || slice.contains('/') {
        return None;
    }
    let parent = show_value(tools, &slice, "ControlGroup").filter(|path| valid_cgroup(path))?;
    Some(format!("{}/{unit}", parent.trim_end_matches('/')))
}

fn show_value(tools: &dyn ServiceTools, unit: &str, property: &str) -> Option<String> {
    let property = format!("--property={property}");
    let output = systemctl(tools, &["show", &property, "--value", unit]).ok()?;
    let value = output.stdout.trim();
    (output.success && !value.is_empty()).then(|| value.to_owned())
}

fn valid_cgroup(path: &str) -> bool {
    path.starts_with('/') && !path.split('/').any(|part| part == "..")
}

/// The processes left in `cgroup` and below it, by PID with their command names. The unified
/// hierarchy is read first, then systemd's own on a legacy (v1) layout.
fn remaining_processes(setup: &ServiceSetup, cgroup: &str) -> Vec<(u32, String)> {
    let relative = cgroup.trim_start_matches('/');
    let mut pids = Vec::new();
    for root in [
        setup.cgroup_root.join(relative),
        setup.cgroup_root.join("systemd").join(relative),
    ] {
        collect_pids(&root, &mut pids);
        if !pids.is_empty() {
            break;
        }
    }
    pids.sort_unstable();
    pids.dedup();
    pids.into_iter()
        .map(|pid| {
            let name = fs::read_to_string(setup.proc_root.join(pid.to_string()).join("comm"))
                .map(|name| name.trim().to_owned())
                .ok()
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "unknown".to_owned());
            (pid, name)
        })
        .collect()
}

fn collect_pids(directory: &Path, pids: &mut Vec<u32>) {
    if let Ok(text) = fs::read_to_string(directory.join("cgroup.procs")) {
        pids.extend(
            text.lines()
                .filter_map(|line| line.trim().parse::<u32>().ok()),
        );
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            collect_pids(&entry.path(), pids);
        }
    }
}

/// `systemctl --user is-active`: `active`, `inactive`, `failed`, `activating`, ...
fn active_state(setup: &ServiceSetup, tools: &dyn ServiceTools) -> Result<String, String> {
    // is-active exits non-zero for every state but active, and still prints the state.
    let output = systemctl(tools, &["is-active", &unit_name(setup.channel)])?;
    Ok(first_word(&output.stdout).unwrap_or("unknown").to_owned())
}

fn first_word(text: &str) -> Option<&str> {
    text.split_whitespace().next()
}

/// `yes`, `no`, or `unknown` when logind does not answer.
pub fn linger(setup: &ServiceSetup, tools: &dyn ServiceTools) -> &'static str {
    let uid = setup.uid.to_string();
    let Ok(output) = tools.run("loginctl", &["show-user", &uid, "--property=Linger"]) else {
        return "unknown";
    };
    if !output.success {
        return "unknown";
    }
    match output
        .stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix("Linger="))
    {
        Some("yes") => "yes",
        Some("no") => "no",
        _ => "unknown",
    }
}

/// Prints the status word (macOS's vocabulary), then `state:`, `unit:`, `program:` and
/// `linger:` lines. Advice on lingering goes to `err`.
fn report(
    setup: &ServiceSetup,
    tools: &dyn ServiceTools,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), String> {
    let path = setup.unit_path();
    let unit = unit_name(setup.channel);
    let (status, state, current) = match read_unit(&path)? {
        UnitFile::Missing => ("notRegistered", "inactive".to_owned(), None),
        UnitFile::Silo(contents) => {
            let enabled = systemctl(tools, &["is-enabled", &unit])?;
            let status = match first_word(&enabled.stdout) {
                Some("enabled") => "enabled",
                _ => "disabled",
            };
            let current = contents == unit_file(setup.channel, &setup.executable)?;
            (status, active_state(setup, tools)?, Some(current))
        }
    };
    let linger = linger(setup, tools);
    let mut text = format!("{status}\nstate: {state}\nunit: {}\n", path.display());
    match current {
        Some(true) => text.push_str("program: this silod\n"),
        Some(false) => text.push_str(&format!(
            "program: another silod; run {} --service register\n",
            setup.executable.display()
        )),
        None => {}
    }
    text.push_str(&format!("linger: {linger}\n"));
    out.write_all(text.as_bytes())
        .map_err(|error| error.to_string())?;
    if status == "enabled" && linger != "yes" {
        let known = if linger == "no" {
            format!("Lingering is off for {}", setup.user)
        } else {
            format!("logind did not say whether {} lingers", setup.user)
        };
        let _ = writeln!(
            err,
            "{known}: the engine starts at login and stops after the last logout. The last \
             logout also ends the user manager, which kills any computer still running under it \
             (systemd's user@.service uses KillMode=mixed; check your distribution's). To start \
             the engine at boot and keep it and its computers running without a session, an \
             administrator runs: sudo loginctl enable-linger {}",
            setup.user
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::HashMap};

    /// Answers like a user manager that keeps each unit's enabled and active state.
    #[derive(Default)]
    struct FakeTools {
        calls: RefCell<Vec<String>>,
        enabled: RefCell<HashMap<String, bool>>,
        active: RefCell<HashMap<String, &'static str>>,
        linger: Option<&'static str>,
        refuse: Option<&'static str>,
        /// What `systemctl show --property=ControlGroup` answers for an active unit.
        cgroup: Option<String>,
    }

    impl ServiceTools for FakeTools {
        fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, String> {
            let line = format!("{program} {}", args.join(" "));
            self.calls.borrow_mut().push(line.clone());
            let answer = |stdout: &str, success: bool| {
                Ok(CommandOutput {
                    success,
                    stdout: format!("{stdout}\n"),
                    stderr: String::new(),
                })
            };
            if self.refuse.is_some_and(|refused| line.contains(refused)) {
                return Ok(CommandOutput {
                    success: false,
                    stdout: String::new(),
                    stderr: "Failed to connect to bus: No medium found\n".into(),
                });
            }
            match (program, args) {
                ("loginctl", ["show-user", "1000", "--property=Linger"]) => match self.linger {
                    Some(value) => answer(&format!("Linger={value}"), true),
                    None => answer("", false),
                },
                ("systemctl", ["--user", "daemon-reload"]) => answer("", true),
                // Like systemd: an active unit names its cgroup, an inactive one does not, and
                // every unit is in app.slice, whose cgroup is the parent of `cgroup`.
                ("systemctl", ["--user", "show", "--property=ControlGroup", "--value", unit]) => {
                    let active = self.active.borrow().get(*unit).copied() == Some("active");
                    match &self.cgroup {
                        Some(cgroup) if *unit == "app.slice" => {
                            answer(cgroup.rsplit_once('/').unwrap().0, true)
                        }
                        Some(cgroup) if active => answer(cgroup, true),
                        _ => answer("", true),
                    }
                }
                ("systemctl", ["--user", "show", "--property=Slice", "--value", _]) => {
                    answer("app.slice", true)
                }
                ("systemctl", ["--user", "reset-failed", _]) => answer("", true),
                ("systemctl", ["--user", "enable", "--now", unit]) => {
                    self.enabled.borrow_mut().insert(unit.to_string(), true);
                    self.active.borrow_mut().insert(unit.to_string(), "active");
                    answer("", true)
                }
                ("systemctl", ["--user", "disable", "--now", unit]) => {
                    self.enabled.borrow_mut().insert(unit.to_string(), false);
                    self.active
                        .borrow_mut()
                        .insert(unit.to_string(), "inactive");
                    answer("", true)
                }
                ("systemctl", ["--user", "is-enabled", unit]) => {
                    if self.enabled.borrow().get(*unit).copied().unwrap_or(false) {
                        answer("enabled", true)
                    } else {
                        answer("disabled", false)
                    }
                }
                ("systemctl", ["--user", "is-active", unit]) => {
                    let state = self
                        .active
                        .borrow()
                        .get(*unit)
                        .copied()
                        .unwrap_or("inactive");
                    answer(state, state == "active")
                }
                _ => panic!("unexpected call: {line}"),
            }
        }
    }

    fn setup(root: &Path, channel: Channel) -> ServiceSetup {
        ServiceSetup {
            channel,
            executable: PathBuf::from("/opt/silo engine/silod"),
            unit_dir: root.join(".config/systemd/user"),
            uid: 1000,
            user: "runner".into(),
            cgroup_root: root.join("cgroup"),
            proc_root: root.join("proc"),
        }
    }

    fn run(
        command: &str,
        setup: &ServiceSetup,
        tools: &FakeTools,
    ) -> (Result<i32, String>, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let result = run_with(command, setup, tools, &mut out, &mut err);
        (
            result,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn unit_names_are_derived_from_the_channel() {
        assert_eq!(
            unit_name(Channel::Development),
            "org.silo.dev.engine.service"
        );
        assert_eq!(
            unit_name(Channel::Production),
            "org.silo.preview.engine.service"
        );
        // The LaunchAgent's label (`service::engine_bundle_identifier` on macOS).
        assert_eq!(
            engine_service_name(Channel::Production),
            "org.silo.preview.engine"
        );
    }

    #[test]
    fn the_unit_starts_the_agent_and_restarts_it_only_after_a_failure() {
        let unit = unit_file(Channel::Development, Path::new("/opt/silo engine/silod")).unwrap();
        let lines: Vec<&str> = unit.lines().collect();
        assert_eq!(lines[0], MARKER);
        for expected in [
            "Description=Silo Dev engine (silod)",
            "ExecStart=\"/opt/silo engine/silod\" --agent",
            "Restart=on-failure",
            "StartLimitIntervalSec=0",
            "UMask=0077",
            "KillMode=process",
            "KillSignal=SIGTERM",
            "TimeoutStopSec=30",
            "WantedBy=default.target",
        ] {
            assert!(lines.contains(&expected), "{expected} missing from\n{unit}");
        }
        // Restart=on-failure restarts after any non-zero exit, so the restart code must be
        // one, and nothing may declare it a success.
        assert_ne!(crate::process::RESTART_EXIT_CODE, 0);
        assert!(!unit.contains("SuccessExitStatus"));
        assert!(!unit.contains("RestartPreventExitStatus"));
        // The rate limit is off, in [Unit] where systemd reads it.
        let unit_section = unit.split("[Service]").next().unwrap();
        assert!(
            unit_section.contains("\nStartLimitIntervalSec=0\n"),
            "{unit}"
        );
        // The session-end shutdown, and the check for computers it left running, fit in the
        // stop timeout, after which systemd sends SIGKILL to silod alone.
        assert!(
            (silo_engine::engine_update::SESSION_END_BUDGET
                + crate::methods::services::SURVIVOR_CHECK
                + std::time::Duration::from_secs(1))
            .as_secs()
                < 30
        );
        assert!(unit_file(Channel::Production, Path::new("/opt/silod"))
            .unwrap()
            .contains("Description=Silo engine (silod)\n"));
    }

    #[test]
    fn exec_start_escapes_what_systemd_would_expand() {
        assert_eq!(
            exec_argument(Path::new("/home/a\"b/%h/$HOME/c\\d/silod")).unwrap(),
            "\"/home/a\\\"b/%%h/$$HOME/c\\\\d/silod\""
        );
        assert!(exec_argument(Path::new("relative/silod")).is_err());
        assert!(exec_argument(Path::new("/tmp/new\nline/silod")).is_err());
    }

    #[test]
    fn the_unit_directory_follows_xdg_config_home() {
        fn lookup(
            pairs: &'static [(&'static str, &'static str)],
        ) -> impl Fn(&str) -> Option<std::ffi::OsString> {
            move |name| {
                pairs
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| std::ffi::OsString::from(value))
            }
        }
        assert_eq!(
            user_unit_dir(lookup(&[("HOME", "/home/u")])).unwrap(),
            PathBuf::from("/home/u/.config/systemd/user")
        );
        assert_eq!(
            user_unit_dir(lookup(&[("HOME", "/home/u"), ("XDG_CONFIG_HOME", "/cfg")])).unwrap(),
            PathBuf::from("/cfg/systemd/user")
        );
        assert_eq!(
            user_unit_dir(lookup(&[("HOME", "/home/u"), ("XDG_CONFIG_HOME", "rel")])).unwrap(),
            PathBuf::from("/home/u/.config/systemd/user")
        );
        assert!(user_unit_dir(lookup(&[])).is_err());
    }

    #[test]
    fn register_writes_the_unit_then_reloads_and_enables_it() {
        let root = tempfile::tempdir().unwrap();
        let setup = setup(root.path(), Channel::Development);
        let tools = FakeTools {
            linger: Some("yes"),
            ..Default::default()
        };
        let (result, out, err) = run("register", &setup, &tools);
        assert_eq!(result, Ok(0));
        let path = setup.unit_path();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            unit_file(Channel::Development, &setup.executable).unwrap()
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o644
        );
        let calls = tools.calls.borrow().clone();
        let reload = calls
            .iter()
            .position(|call| call == "systemctl --user daemon-reload")
            .unwrap();
        let enable = calls
            .iter()
            .position(|call| call == "systemctl --user enable --now org.silo.dev.engine.service")
            .unwrap();
        assert!(reload < enable, "{calls:?}");
        assert_eq!(
            out,
            format!(
                "enabled\nstate: active\nunit: {}\nprogram: this silod\nlinger: yes\n",
                path.display()
            )
        );
        assert_eq!(err, "");
        // No temporary file is left beside the unit.
        assert_eq!(fs::read_dir(&setup.unit_dir).unwrap().count(), 1);
    }

    #[test]
    fn register_says_how_to_turn_lingering_on_and_never_asks_for_privileges() {
        let root = tempfile::tempdir().unwrap();
        let setup = setup(root.path(), Channel::Production);
        let tools = FakeTools {
            linger: Some("no"),
            ..Default::default()
        };
        let (result, out, err) = run("register", &setup, &tools);
        assert_eq!(result, Ok(0));
        assert!(out.ends_with("linger: no\n"), "{out}");
        assert!(err.contains("sudo loginctl enable-linger runner"), "{err}");
        assert!(
            err.contains("kills any computer still running under it"),
            "{err}"
        );
        assert!(err.contains("KillMode=mixed"), "{err}");
        // logind is asked about the UID, never a name from the environment.
        assert!(tools
            .calls
            .borrow()
            .contains(&"loginctl show-user 1000 --property=Linger".to_owned()));
        assert!(tools
            .calls
            .borrow()
            .iter()
            .all(|call| !call.contains("sudo") && !call.contains("enable-linger")));
        // logind not answering is reported as unknown, not as an error.
        let (result, out, _) = run("status", &setup, &FakeTools::default());
        assert_eq!(result, Ok(0));
        assert!(out.ends_with("linger: unknown\n"), "{out}");
    }

    #[test]
    fn a_file_silod_did_not_write_is_never_replaced_or_removed() {
        let root = tempfile::tempdir().unwrap();
        let setup = setup(root.path(), Channel::Development);
        fs::create_dir_all(&setup.unit_dir).unwrap();
        let path = setup.unit_path();
        let foreign = "[Service]\nExecStart=/usr/bin/true\n";
        fs::write(&path, foreign).unwrap();
        for command in ["register", "unregister", "status"] {
            let tools = FakeTools::default();
            let error = run(command, &setup, &tools).0.unwrap_err();
            assert!(
                error.contains("was not written by silod"),
                "{command}: {error}"
            );
            assert!(
                tools.calls.borrow().is_empty(),
                "{command}: {:?}",
                tools.calls.borrow()
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), foreign);
        }
        // Nor a link at that path, whatever it points to.
        fs::remove_file(&path).unwrap();
        let target = root.path().join("target.service");
        fs::write(
            &target,
            unit_file(Channel::Development, &setup.executable).unwrap(),
        )
        .unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        let error = run("register", &setup, &FakeTools::default())
            .0
            .unwrap_err();
        assert!(error.contains("was not written by silod"), "{error}");
        assert!(fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[test]
    fn registering_from_another_path_rewrites_the_unit_and_says_to_restart() {
        let root = tempfile::tempdir().unwrap();
        let mut setup = setup(root.path(), Channel::Development);
        let tools = FakeTools {
            linger: Some("yes"),
            ..Default::default()
        };
        assert_eq!(run("register", &setup, &tools).0, Ok(0));
        setup.executable = PathBuf::from("/opt/silo-2/silod");
        let (_, out, _) = run("status", &setup, &tools);
        assert!(
            out.contains("program: another silod; run /opt/silo-2/silod --service register"),
            "{out}"
        );
        let (result, out, err) = run("register", &setup, &tools);
        assert_eq!(result, Ok(0));
        assert!(out.contains("program: this silod"), "{out}");
        assert!(
            err.contains("systemctl --user restart org.silo.dev.engine.service"),
            "{err}"
        );
        assert!(fs::read_to_string(setup.unit_path())
            .unwrap()
            .contains("ExecStart=\"/opt/silo-2/silod\" --agent"));
        // An unchanged unit is left as it is and needs no restart.
        let (_, _, err) = run("register", &setup, &tools);
        assert_eq!(err, "");
    }

    #[test]
    fn unregister_disables_stops_and_removes_the_unit() {
        let root = tempfile::tempdir().unwrap();
        let setup = setup(root.path(), Channel::Development);
        let tools = FakeTools {
            linger: Some("no"),
            ..Default::default()
        };
        assert_eq!(run("register", &setup, &tools).0, Ok(0));
        tools.calls.borrow_mut().clear();
        let (result, out, err) = run("unregister", &setup, &tools);
        assert_eq!(result, Ok(0));
        assert!(!setup.unit_path().exists());
        let calls = tools.calls.borrow().clone();
        assert_eq!(
            calls[..6],
            [
                "systemctl --user show --property=ControlGroup --value \
                 org.silo.dev.engine.service",
                "systemctl --user show --property=Slice --value org.silo.dev.engine.service",
                "systemctl --user show --property=ControlGroup --value app.slice",
                "systemctl --user disable --now org.silo.dev.engine.service",
                "systemctl --user reset-failed org.silo.dev.engine.service",
                "systemctl --user daemon-reload"
            ]
        );
        assert!(out.starts_with("notRegistered\nstate: inactive\n"), "{out}");
        assert_eq!(
            err, "",
            "no lingering advice for an engine that is not registered"
        );
        // Unregistering again changes nothing and reports the same.
        tools.calls.borrow_mut().clear();
        let (result, out, _) = run("unregister", &setup, &tools);
        assert_eq!(result, Ok(0));
        assert!(out.starts_with("notRegistered\n"), "{out}");
        assert!(tools
            .calls
            .borrow()
            .iter()
            .all(|call| call.starts_with("loginctl ")));
    }

    #[test]
    fn status_reads_a_unit_that_is_not_enabled_as_disabled() {
        let root = tempfile::tempdir().unwrap();
        let setup = setup(root.path(), Channel::Development);
        write_unit(
            &setup.unit_path(),
            &unit_file(Channel::Development, &setup.executable).unwrap(),
        )
        .unwrap();
        let (result, out, _) = run("status", &setup, &FakeTools::default());
        assert_eq!(result, Ok(0));
        assert!(out.starts_with("disabled\nstate: inactive\n"), "{out}");
    }

    #[test]
    fn an_unreachable_user_manager_is_named_with_a_way_out() {
        let root = tempfile::tempdir().unwrap();
        let setup = setup(root.path(), Channel::Development);
        let tools = FakeTools {
            refuse: Some("daemon-reload"),
            ..Default::default()
        };
        let error = run("register", &setup, &tools).0.unwrap_err();
        assert!(
            error.contains("systemctl --user daemon-reload failed: Failed to connect to bus"),
            "{error}"
        );
        assert!(error.contains("XDG_RUNTIME_DIR"), "{error}");
    }

    #[test]
    fn commands_are_checked_before_anything_is_read() {
        assert!(run_command(&[])
            .unwrap_err()
            .starts_with("usage: silod --service status|register|unregister"));
        assert_eq!(
            run_command(&["status".into(), "extra".into()]).unwrap_err(),
            "--service accepts exactly one command"
        );
        assert!(run_command(&["open-settings".into()])
            .unwrap_err()
            .contains("macOS only"));
        assert_eq!(
            run_command(&["start".into()]).unwrap_err(),
            "unsupported service command: start"
        );
    }

    #[test]
    fn unregister_names_the_computer_processes_the_session_end_left_running() {
        let root = tempfile::tempdir().unwrap();
        let setup = setup(root.path(), Channel::Development);
        let cgroup =
            "/user.slice/user-1000.slice/user@1000.service/app.slice/org.silo.dev.engine.service";
        let tools = FakeTools {
            linger: Some("yes"),
            cgroup: Some(cgroup.into()),
            ..Default::default()
        };
        assert_eq!(run("register", &setup, &tools).0, Ok(0));
        // Nothing left behind: nothing to say.
        let (result, _, err) = run("unregister", &setup, &tools);
        assert_eq!(result, Ok(0));
        assert_eq!(err, "");

        assert_eq!(run("register", &setup, &tools).0, Ok(0));
        let directory = setup.cgroup_root.join(cgroup.trim_start_matches('/'));
        fs::create_dir_all(directory.join("nested")).unwrap();
        fs::write(directory.join("cgroup.procs"), "4242\n").unwrap();
        fs::write(directory.join("nested/cgroup.procs"), "4343\n").unwrap();
        fs::create_dir_all(setup.proc_root.join("4242")).unwrap();
        fs::write(setup.proc_root.join("4242/comm"), "msb\n").unwrap();
        let (result, out, err) = run("unregister", &setup, &tools);
        assert_eq!(result, Ok(0));
        assert!(out.starts_with("notRegistered\n"), "{out}");
        assert!(err.contains("msb (PID 4242), unknown (PID 4343)"), "{err}");
        assert!(err.contains("silod --service register"), "{err}");
        assert!(!setup.unit_path().exists());
        assert!(tools
            .calls
            .borrow()
            .contains(&"systemctl --user reset-failed org.silo.dev.engine.service".to_owned()));

        // A unit that is already inactive (its stop failed, the survivors stayed) names no
        // cgroup; the path comes from its slice, and the survivors are still named.
        assert_eq!(run("register", &setup, &tools).0, Ok(0));
        tools
            .active
            .borrow_mut()
            .insert(unit_name(Channel::Development), "failed");
        let (result, _, err) = run("unregister", &setup, &tools);
        assert_eq!(result, Ok(0));
        assert!(err.contains("msb (PID 4242), unknown (PID 4343)"), "{err}");
    }

    #[test]
    fn the_account_name_comes_from_the_user_database() {
        let uid = unsafe { libc::getuid() };
        let name = account_name(uid);
        assert!(
            !name.is_empty() && !name.contains(char::is_whitespace),
            "{name}"
        );
        let expected = Command::new("id").arg("-un").output().unwrap();
        if expected.status.success() {
            assert_eq!(name, String::from_utf8_lossy(&expected.stdout).trim());
        }
        // A UID the database does not know is named by its number.
        assert_eq!(account_name(3_999_999_999), "3999999999");
    }

    #[test]
    fn a_fifo_at_the_unit_path_is_refused_without_blocking() {
        let root = tempfile::tempdir().unwrap();
        let setup = setup(root.path(), Channel::Development);
        fs::create_dir_all(&setup.unit_dir).unwrap();
        let path = std::ffi::CString::new(setup.unit_path().to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        let error = run("status", &setup, &FakeTools::default()).0.unwrap_err();
        assert!(error.contains("was not written by silod"), "{error}");
    }

    #[test]
    fn a_temporary_file_a_crashed_run_left_is_replaced_only_when_it_is_silods() {
        let root = tempfile::tempdir().unwrap();
        let setup = setup(root.path(), Channel::Development);
        fs::create_dir_all(&setup.unit_dir).unwrap();
        let temporary = setup.unit_dir.join(format!(
            ".{}.{}.tmp",
            unit_name(Channel::Development),
            std::process::id()
        ));
        fs::write(&temporary, format!("{MARKER}\n[Unit]\n")).unwrap();
        let tools = FakeTools {
            linger: Some("yes"),
            ..Default::default()
        };
        assert_eq!(run("register", &setup, &tools).0, Ok(0));
        assert!(!temporary.exists());
        assert_eq!(fs::read_dir(&setup.unit_dir).unwrap().count(), 1);

        // Someone else's file under that name is left alone, and nothing is written.
        fs::remove_file(setup.unit_path()).unwrap();
        fs::write(&temporary, "not silod's\n").unwrap();
        let error = run("register", &setup, &tools).0.unwrap_err();
        assert!(error.starts_with("Cannot write"), "{error}");
        assert_eq!(fs::read_to_string(&temporary).unwrap(), "not silod's\n");
        assert!(!setup.unit_path().exists());
    }
}
