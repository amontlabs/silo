//! The guest commands a keep runs. Values travel as `A_*` environment variables; the
//! scripts are fixed text. Linux and macOS differ in `stat`, hashing, package managers and
//! how services restart.

use super::env::{GuestCmd, Os};

const PRELUDE: &str = "export PATH=\"$PATH:/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:$HOME/.cargo/bin:$HOME/.local/bin\"\nset -u\n";

fn cmd(tag: &'static str, body: &str) -> GuestCmd {
    GuestCmd::new(tag, format!("{PRELUDE}{body}"))
}

fn stat_mode(os: Os) -> &'static str {
    match os {
        Os::Linux => "stat -c %a",
        Os::Macos => "stat -f %Lp",
    }
}

fn stat_owner(os: Os) -> &'static str {
    match os {
        Os::Linux => "stat -c %u:%g",
        Os::Macos => "stat -f %u:%g",
    }
}

fn sha(os: Os) -> &'static str {
    match os {
        Os::Linux => "sha256sum",
        Os::Macos => "shasum -a 256",
    }
}

pub fn file_mode(os: Os, path: &str) -> GuestCmd {
    cmd("file_mode", &format!("{} -- \"$A_path\"", stat_mode(os))).arg("path", path)
}

pub fn file_read(path: &str) -> GuestCmd {
    let mut c = cmd("file_read", "cat -- \"$A_path\"").arg("path", path);
    c.timeout = std::time::Duration::from_secs(175);
    c
}

pub fn file_write(os: Os, path: &str, mode: &str, data: &[u8]) -> GuestCmd {
    let body = format!(
        "set -e\nd=$(dirname -- \"$A_path\")\nmkdir -p -- \"$d\"\nt=\"$A_path.silo-keep.$$\"\ncat > \"$t\"\nchmod \"$A_mode\" \"$t\"\nif [ -e \"$A_path\" ]; then o=$({owner} -- \"$A_path\"); else o=$({owner} -- \"$d\"); fi\nchown \"$o\" \"$t\" 2>/dev/null || true\nmv -f -- \"$t\" \"$A_path\"",
        owner = stat_owner(os)
    );
    cmd("file_write", &body)
        .arg("path", path)
        .arg("mode", mode)
        .stdin(data.to_vec())
}

/// Reads one path per line from stdin; prints `path<TAB>sha256` or `path<TAB>L:target`.
pub fn fingerprint(os: Os, paths: &[String]) -> GuestCmd {
    let body = format!(
        "while IFS= read -r p; do if [ -L \"$p\" ]; then printf '%s\\tL:%s\\n' \"$p\" \"$(readlink \"$p\")\"; elif [ -f \"$p\" ]; then printf '%s\\t%s\\n' \"$p\" \"$({sha} < \"$p\" | cut -d' ' -f1)\"; fi; done",
        sha = sha(os)
    );
    cmd("fingerprint", &body).stdin(paths.join("\n").into_bytes())
}

pub fn file_remove(path: &str) -> GuestCmd {
    cmd("file_remove", "rm -f -- \"$A_path\"").arg("path", path)
}

pub fn symlink(path: &str, target: &str) -> GuestCmd {
    cmd(
        "symlink",
        "mkdir -p -- \"$(dirname -- \"$A_path\")\"\nln -sfn -- \"$A_target\" \"$A_path\"",
    )
    .arg("path", path)
    .arg("target", target)
}

/// `specs` is one install spec per line, in the manager's own syntax.
pub fn pkg_install(os: Os, manager: &str, specs: &[String]) -> Result<GuestCmd, String> {
    let install = match (manager, os) {
        ("dpkg", Os::Linux) => "DEBIAN_FRONTEND=noninteractive apt-get install -y -qq \"${S[@]}\" >/dev/null",
        ("brew", _) => "brew install \"${S[@]}\"",
        ("brew-cask", _) => "brew install --cask \"${S[@]}\"",
        ("npm", _) => "npm install -g --silent \"${S[@]}\"",
        ("pip", Os::Linux) => "python3 -m pip install -q --break-system-packages \"${S[@]}\"",
        ("pip", Os::Macos) => "python3 -m pip install -q --break-system-packages \"${S[@]}\"",
        ("pipx", _) => "for s in \"${S[@]}\"; do pipx install --force \"$s\"; done",
        ("cargo", _) => "for s in \"${S[@]}\"; do cargo install --locked \"${s%%=*}\" --version \"${s#*=}\"; done",
        _ => return Err(format!("{manager} packages cannot be replayed here.")),
    };
    let body = format!(
        "set -e\nS=()\nwhile IFS= read -r s; do [ -n \"$s\" ] && S+=(\"$s\"); done <<<\"$A_specs\"\n{install}"
    );
    let mut c = cmd("pkg_install", &body)
        .arg("manager", manager)
        .arg("specs", specs.join("\n"));
    c.timeout = std::time::Duration::from_secs(900);
    Ok(c)
}

pub fn git_bundle(repo: &str, bundle: &str) -> GuestCmd {
    cmd(
        "git_bundle",
        "set -e\ncd \"$A_repo\"\ngit bundle create \"$A_bundle\" HEAD >/dev/null 2>&1",
    )
    .arg("repo", repo)
    .arg("bundle", bundle)
}

/// Prints `uptodate`, `ff`, `merge`, `conflict` (then the conflict lines) or `unsupported`.
pub fn git_precheck(repo: &str, bundle: &str) -> GuestCmd {
    cmd(
        "git_precheck",
        "set -e\ncd \"$A_repo\"\ngit fetch -q \"$A_bundle\" HEAD:refs/silo/keep\nif git merge-base --is-ancestor refs/silo/keep HEAD; then echo uptodate; exit 0; fi\nif git merge-base --is-ancestor HEAD refs/silo/keep; then echo ff; exit 0; fi\nif out=$(git merge-tree --write-tree HEAD refs/silo/keep 2>&1); then echo merge; else if echo \"$out\" | grep -q CONFLICT; then echo conflict; echo \"$out\" | grep CONFLICT; else echo unsupported; fi; fi",
    )
    .arg("repo", repo)
    .arg("bundle", bundle)
}

pub fn git_merge(repo: &str) -> GuestCmd {
    cmd(
        "git_merge",
        "cd \"$A_repo\"\nif git merge --ff-only -q refs/silo/keep 2>/dev/null; then :; elif git -c user.name=Silo -c user.email=silo@localhost merge --no-edit -q refs/silo/keep; then :; else git merge --abort 2>/dev/null; echo 'merge conflict' >&2; exit 3; fi\ngit rev-parse --short HEAD",
    )
    .arg("repo", repo)
}

pub fn git_clone(repo: &str, bundle: &str) -> GuestCmd {
    cmd(
        "git_clone",
        "set -e\nmkdir -p -- \"$(dirname -- \"$A_repo\")\"\ngit clone -q \"$A_bundle\" \"$A_repo\"\ncd \"$A_repo\"\ngit rev-parse --short HEAD",
    )
    .arg("repo", repo)
    .arg("bundle", bundle)
}

pub fn git_changed(repo: &str, old: &str, new: &str) -> GuestCmd {
    cmd(
        "git_changed",
        "cd \"$A_repo\" && git diff --name-only \"$A_old\" \"$A_new\"",
    )
    .arg("repo", repo)
    .arg("old", old)
    .arg("new", new)
}

pub fn crontab_read(user: &str) -> GuestCmd {
    cmd(
        "crontab_read",
        "e=$(mktemp)\ncrontab -u \"$A_user\" -l 2>\"$e\" || { grep -q 'no crontab' \"$e\" || { cat \"$e\" >&2; exit 1; }; }\nrm -f \"$e\"",
    )
    .arg("user", user)
}

pub fn crontab_write(user: &str, text: &str) -> GuestCmd {
    cmd("crontab_write", "crontab -u \"$A_user\" -")
        .arg("user", user)
        .stdin(text.as_bytes().to_vec())
}

/// Loads a LaunchAgent or daemon; a failure is the caller's to report, not fatal.
pub fn agent_load(scope: &str, label: &str, path: &str) -> GuestCmd {
    cmd(
        "agent_load",
        "if [ \"$A_scope\" = system ]; then d=system; else d=gui/$(stat -f %u /dev/console); fi\nlaunchctl bootout \"$d/$A_label\" 2>/dev/null || true\nlaunchctl bootstrap \"$d\" \"$A_path\"",
    )
    .arg("scope", scope)
    .arg("label", label)
    .arg("path", path)
}

pub fn agent_unload(scope: &str, label: &str) -> GuestCmd {
    cmd(
        "agent_unload",
        "if [ \"$A_scope\" = system ]; then d=system; else d=gui/$(stat -f %u /dev/console); fi\nlaunchctl bootout \"$d/$A_label\" 2>/dev/null || true",
    )
    .arg("scope", scope)
    .arg("label", label)
}

/// Prints the main process id of the service, or 0.
pub fn service_pid(os: Os, name: &str) -> GuestCmd {
    let body = match os {
        Os::Linux => "p=$(systemctl show -p MainPID --value \"$A_name\" 2>/dev/null || true)\nif [ -z \"$p\" ] || [ \"$p\" = 0 ]; then p=$(pidof -s \"$A_name\" 2>/dev/null || true); fi\necho \"${p:-0}\"",
        Os::Macos => "u=$(stat -f %u /dev/console)\np=$( { launchctl print \"gui/$u/$A_name\" 2>/dev/null || launchctl print \"system/$A_name\" 2>/dev/null; } | sed -n 's/^[[:space:]]*pid = \\([0-9][0-9]*\\)$/\\1/p' | head -1)\necho \"${p:-0}\"",
    };
    cmd("service_pid", body).arg("name", name)
}

pub fn service_restart(os: Os, name: &str) -> GuestCmd {
    let body = match os {
        Os::Linux => "if systemctl cat \"$A_name\" >/dev/null 2>&1; then systemctl restart \"$A_name\"; else service \"$A_name\" restart; fi",
        Os::Macos => "u=$(stat -f %u /dev/console)\nlaunchctl kickstart -k \"gui/$u/$A_name\" || launchctl kickstart -k \"system/$A_name\"",
    };
    cmd("service_restart", body).arg("name", name)
}

pub fn pid_alive(pid: &str) -> GuestCmd {
    cmd("pid_alive", "kill -0 \"$A_pid\"").arg("pid", pid)
}

/// A recipe hook, run in `cwd` as the working account.
pub fn hook(command: &str, cwd: &str) -> GuestCmd {
    let mut c = cmd("hook", "cd \"$A_cwd\" && bash -lc \"$A_cmd\"")
        .arg("cmd", command)
        .arg("cwd", cwd);
    c.timeout = std::time::Duration::from_secs(900);
    c
}
