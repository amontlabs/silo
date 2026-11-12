//! The guest commands a keep runs. Values travel as `A_*` environment variables; the
//! scripts are fixed text. Linux and macOS differ in `stat`, hashing, package managers and
//! how services restart.

use std::time::Duration;

use super::env::{GuestCmd, Os, RunAs};

const PRELUDE: &str = "export PATH=\"$PATH:/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/home/linuxbrew/.linuxbrew/bin:$HOME/.cargo/bin:$HOME/.local/bin\"\nset -u\n";

fn cmd(tag: &'static str, body: &str) -> GuestCmd {
    GuestCmd::new(tag, format!("{PRELUDE}{body}"))
}

/// Runs what follows as the owner of `$1` when the script is root and that owner is another
/// account (the collector reads as root, but a repository belongs to its owner, and git
/// refuses a repository owned by someone else). `git` below is that, for `$A_repo`.
const AS_OWNER: &str = "as_owner() {\n  o=$(stat -c %U -- \"$1\" 2>/dev/null || stat -f %Su -- \"$1\" 2>/dev/null)\n  shift\n  if [ \"$(id -u)\" = 0 ] && [ -n \"$o\" ] && [ \"$o\" != root ]; then\n    h=$(getent passwd \"$o\" 2>/dev/null | cut -d: -f6); [ -n \"$h\" ] || h=$(eval echo \"~$o\")\n    if command -v runuser >/dev/null 2>&1; then runuser -u \"$o\" -- env HOME=\"$h\" \"$@\"; else sudo -n -u \"$o\" -H \"$@\"; fi\n  else command \"$@\"; fi\n}\ngit() { as_owner \"$A_repo\" git \"$@\"; }\n";

fn git_cmd(tag: &'static str, body: &str) -> GuestCmd {
    cmd(tag, &format!("{AS_OWNER}{body}"))
}

fn stat_mode(os: Os) -> &'static str {
    match os {
        Os::Linux => "stat -c %a",
        Os::Macos => "stat -f %Lp",
    }
}

fn stat_owner(os: Os) -> &'static str {
    match os {
        Os::Linux => "stat -L -c %u:%g",
        Os::Macos => "stat -L -f %u:%g",
    }
}

fn sha(os: Os) -> &'static str {
    match os {
        Os::Linux => "sha256sum",
        Os::Macos => "shasum -a 256",
    }
}

fn nul_list(paths: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    for path in paths {
        out.extend_from_slice(path.as_bytes());
        out.push(0);
    }
    out
}

/// On macOS `/etc`, `/var` and `/tmp` are links into `/private`; keeps address the real place.
pub fn normalize(os: Os, path: &str) -> String {
    if os == Os::Macos {
        for root in ["/etc", "/var", "/tmp"] {
            if path == root || path.starts_with(&format!("{root}/")) {
                return format!("/private{path}");
            }
        }
    }
    path.to_owned()
}

/// Paths in (NUL-delimited), records out: path, kind (`F`, `L`, `D`, `X` missing, `E`
/// unreadable), mode, sha256 or link target. Links are reported, never followed.
pub fn inspect(os: Os, paths: &[String]) -> GuestCmd {
    let body = format!(
        "while IFS= read -r -d '' p; do\n  if [ -L \"$p\" ]; then printf '%s\\0L\\0-\\0%s\\0' \"$p\" \"$(readlink -- \"$p\")\"\n  elif [ -d \"$p\" ]; then printf '%s\\0D\\0-\\0-\\0' \"$p\"\n  elif [ -f \"$p\" ]; then\n    if [ -r \"$p\" ]; then m=$({mode} \"$p\"); h=$({sha} < \"$p\" | cut -d' ' -f1); printf '%s\\0F\\0%s\\0%s\\0' \"$p\" \"$m\" \"$h\"; else printf '%s\\0E\\0-\\0-\\0' \"$p\"; fi\n  elif [ -e \"$p\" ]; then printf '%s\\0E\\0-\\0-\\0' \"$p\"\n  else printf '%s\\0X\\0-\\0-\\0' \"$p\"; fi\ndone",
        mode = stat_mode(os),
        sha = sha(os)
    );
    cmd("inspect", &body).stdin(nul_list(paths))
}

/// Paths in (NUL-delimited), pairs out: path, `ok` or why a keep may not write there.
pub fn dest_check(paths: &[String]) -> GuestCmd {
    let body = "while IFS= read -r -d '' p; do\n  r=ok\n  d=$(dirname -- \"$p\")\n  while [ \"$d\" != / ] && [ \"$d\" != . ]; do\n    if [ -L \"$d\" ]; then r=\"$d is a link\"; break; fi\n    if [ -e \"$d\" ] && [ ! -d \"$d\" ]; then r=\"$d is not a folder\"; break; fi\n    d=$(dirname -- \"$d\")\n  done\n  if [ \"$r\" = ok ] && [ -d \"$p\" ] && [ ! -L \"$p\" ]; then r=\"$p is a folder\"; fi\n  printf '%s\\0%s\\0' \"$p\" \"$r\"\ndone";
    cmd("dest_check", body).stdin(nul_list(paths))
}

pub fn parse_pairs(bytes: &[u8]) -> Vec<(String, String)> {
    let fields: Vec<&[u8]> = bytes.split(|b| *b == 0).collect();
    fields
        .chunks_exact(2)
        .map(|pair| {
            (
                String::from_utf8_lossy(pair[0]).into_owned(),
                String::from_utf8_lossy(pair[1]).into_owned(),
            )
        })
        .collect()
}

pub fn file_read(path: &str) -> GuestCmd {
    let mut c = cmd(
        "file_read",
        "if [ -L \"$A_path\" ] || [ ! -f \"$A_path\" ]; then echo 'not a regular file' >&2; exit 3; fi\ncat -- \"$A_path\"",
    )
    .arg("path", path);
    c.timeout = Duration::from_secs(175);
    c
}

pub fn file_write(os: Os, path: &str, mode: &str, data: &[u8]) -> GuestCmd {
    // The parents are the account's to replace, so a link is never chowned and the temporary file
    // is made exclusively inside the parent. Ownership is set before the mode, which would
    // otherwise lose a setuid or setgid bit.
    let body = format!(
        "set -e\nd=$(dirname -- \"$A_path\")\na=\"$d\"; while [ ! -d \"$a\" ]; do a=$(dirname -- \"$a\"); done\nao=$({owner} -- \"$a\")\nmkdir -p -- \"$d\"\nx=\"$d\"; while [ \"$x\" != \"$a\" ]; do if [ -d \"$x\" ] && [ ! -L \"$x\" ]; then chown -h \"$ao\" \"$x\" 2>/dev/null || true; fi; x=$(dirname -- \"$x\"); done\nt=$(mktemp \"$d/.silo-keep.XXXXXX\")\ntrap 'rm -f \"$t\"' EXIT\ncat > \"$t\"\nif [ -e \"$A_path\" ] && [ ! -L \"$A_path\" ]; then o=$({owner} -- \"$A_path\"); else o=\"$ao\"; fi\nchown -h \"$o\" \"$t\" 2>/dev/null || true\nchmod \"$A_mode\" \"$t\"\nmv -f -- \"$t\" \"$A_path\"\ntrap - EXIT",
        owner = stat_owner(os)
    );
    cmd("file_write", &body)
        .arg("path", path)
        .arg("mode", mode)
        .stdin(data.to_vec())
}

pub fn file_remove(path: &str) -> GuestCmd {
    cmd("file_remove", "rm -f -- \"$A_path\"").arg("path", path)
}

pub fn symlink(path: &str, target: &str) -> GuestCmd {
    cmd(
        "symlink",
        "set -e\nmkdir -p -- \"$(dirname -- \"$A_path\")\"\nt=\"$A_path.silo-keep.$$\"\ntrap 'rm -f \"$t\"' EXIT\nln -s -- \"$A_target\" \"$t\"\nmv -f -- \"$t\" \"$A_path\"\ntrap - EXIT",
    )
    .arg("path", path)
    .arg("target", target)
}

/// Package and version names a keep will hand to a package manager.
pub fn valid_package_word(word: &str, scoped: bool) -> bool {
    let first = word.chars().next();
    let start_ok = first.is_some_and(|c| c.is_ascii_alphanumeric() || (scoped && c == '@'));
    start_ok
        && word.len() <= 200
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._+-:~@/".contains(c))
        && !word.contains("..")
}

/// Who runs a manager: user-level managers as the working account, system ones as root.
pub fn pkg_runner(os: Os, manager: &str) -> RunAs {
    match (manager, os) {
        ("dpkg", _) => RunAs::Root,
        ("npm" | "pip" | "cargo" | "pipx", Os::Linux) => RunAs::Root,
        _ => RunAs::Account,
    }
}

/// `specs` is one install spec per line, in the manager's own syntax.
pub fn pkg_install(os: Os, manager: &str, specs: &[String]) -> Result<GuestCmd, String> {
    let install = match (manager, os) {
        ("dpkg", Os::Linux) => "DEBIAN_FRONTEND=noninteractive apt-get install -y -qq -- \"${S[@]}\" >/dev/null",
        ("brew", _) => "brew install -- \"${S[@]}\"",
        ("brew-cask", _) => "brew install --cask -- \"${S[@]}\"",
        ("npm", _) => "npm install -g --silent -- \"${S[@]}\"",
        ("pip", _) => "python3 -m pip install -q --break-system-packages -- \"${S[@]}\"",
        ("pipx", _) => "for s in \"${S[@]}\"; do pipx install --force -- \"$s\"; done",
        ("cargo", _) => "for s in \"${S[@]}\"; do cargo install --locked --version \"${s#*=}\" -- \"${s%%=*}\"; done",
        _ => return Err(format!("{manager} packages cannot be replayed here.")),
    };
    let body = format!(
        "set -e\nS=()\nwhile IFS= read -r s; do [ -n \"$s\" ] && S+=(\"$s\"); done <<<\"$A_specs\"\n{install}"
    );
    let mut c = cmd("pkg_install", &body)
        .arg("manager", manager)
        .arg("specs", specs.join("\n"));
    c.timeout = Duration::from_secs(900);
    Ok(c)
}

/// Refuses, with exit code 4, a removal that would take anything beyond the listed packages
/// with it: apt's simulated removal, or the installed formulae that depend on a brew one.
fn remove_guard(manager: &str) -> &'static str {
    match manager {
        "dpkg" => concat!(
            "sim=$(apt-get -s remove -- \"${S[@]}\" 2>/dev/null | sed -n 's/^Remv \\([^ ]*\\).*/\\1/p')\n",
            "extra=\"\"\n",
            "for p in $sim; do b=${p%%:*}; ok=0; for s in \"${S[@]}\"; do [ \"$b\" = \"${s%%:*}\" ] && ok=1; done; [ $ok = 1 ] || extra=\"$extra $b\"; done\n",
            "if [ -n \"$extra\" ]; then echo \"Removing these packages would also remove:$extra\" >&2; exit 4; fi\n",
        ),
        "brew" => concat!(
            "extra=\"\"\n",
            "for s in \"${S[@]}\"; do for u in $(brew uses --installed -- \"$s\" 2>/dev/null); do ok=0; for t in \"${S[@]}\"; do [ \"$u\" = \"$t\" ] && ok=1; done; [ $ok = 1 ] || extra=\"$extra $u\"; done; done\n",
            "if [ -n \"$extra\" ]; then echo \"Other installed packages depend on these:$extra\" >&2; exit 4; fi\n",
        ),
        _ => "",
    }
}

pub fn pkg_remove(os: Os, manager: &str, names: &[String]) -> Result<GuestCmd, String> {
    let remove = match (manager, os) {
        ("dpkg", Os::Linux) => {
            "DEBIAN_FRONTEND=noninteractive apt-get remove -y -qq -- \"${S[@]}\" >/dev/null"
        }
        ("brew", _) => "brew uninstall -- \"${S[@]}\"",
        ("brew-cask", _) => "brew uninstall --cask -- \"${S[@]}\"",
        ("npm", _) => "npm uninstall -g --silent -- \"${S[@]}\"",
        ("pip", _) => "python3 -m pip uninstall -y -q --break-system-packages -- \"${S[@]}\"",
        ("pipx", _) => "for s in \"${S[@]}\"; do pipx uninstall -- \"$s\"; done",
        ("cargo", _) => "for s in \"${S[@]}\"; do cargo uninstall -- \"$s\"; done",
        _ => return Err(format!("{manager} packages cannot be removed here.")),
    };
    let guard = remove_guard(manager);
    let body = format!(
        "set -e\nS=()\nwhile IFS= read -r s; do [ -n \"$s\" ] && S+=(\"$s\"); done <<<\"$A_specs\"\n{guard}{remove}"
    );
    let mut c = cmd("pkg_remove", &body)
        .arg("manager", manager)
        .arg("specs", names.join("\n"));
    c.timeout = Duration::from_secs(900);
    Ok(c)
}

/// The version `brew install name` would install, from the formula or cask.
/// The JSON is flattened and read with `grep -o` and `cut`, which BSD and GNU tools share
/// (BSD `sed` has no `t;`).
pub fn brew_version(name: &str, cask: bool) -> GuestCmd {
    let (flag, pick) = if cask {
        ("--cask", "grep -o '\"version\":\"[^\"]*\"'")
    } else {
        (
            "",
            "grep -o '\"versions\":{[^}]*}' | grep -o '\"stable\":\"[^\"]*\"'",
        )
    };
    cmd(
        "brew_version",
        &format!("brew info --json=v2 {flag} -- \"$A_name\" 2>/dev/null | tr -d '\\n ' | {pick} | head -1 | cut -d'\"' -f4; true"),
    )
    .arg("name", name)
}

pub fn git_bundle(repo: &str, bundle: &str) -> GuestCmd {
    git_cmd(
        "git_bundle",
        "set -e\ncd \"$A_repo\"\ngit bundle create \"$A_bundle\" HEAD >/dev/null 2>&1",
    )
    .arg("repo", repo)
    .arg("bundle", bundle)
}

/// Fetches the bundle into a temporary ref and prints `uptodate`, `ff`, `merge`, `conflict`
/// (then the conflict lines) or `unsupported`.
pub fn git_precheck(repo: &str, bundle: &str, tmp_ref: &str) -> GuestCmd {
    git_cmd(
        "git_precheck",
        "set -e\ncd \"$A_repo\"\ngit fetch -q --no-write-fetch-head \"$A_bundle\" \"HEAD:$A_ref\"\nif git merge-base --is-ancestor \"$A_ref\" HEAD; then echo uptodate; exit 0; fi\nif git merge-base --is-ancestor HEAD \"$A_ref\"; then echo ff; exit 0; fi\nif ! git merge-tree --write-tree HEAD HEAD >/dev/null 2>&1; then echo unsupported; exit 0; fi\nset +e\nout=$(git merge-tree --write-tree HEAD \"$A_ref\" 2>&1); rc=$?\nset -e\nif [ $rc -eq 0 ]; then echo merge; elif echo \"$out\" | grep -q CONFLICT; then echo conflict; echo \"$out\" | grep CONFLICT; else echo unsupported; fi",
    )
    .arg("repo", repo)
    .arg("bundle", bundle)
    .arg("ref", tmp_ref)
}

/// Prints the name the fork's commits get as a branch (`name`, or the first of `name-2`,
/// `name-3`, ... that is free) and `new`, or `same` when that branch already points at them.
/// An existing branch is never moved.
pub fn git_branch_plan(repo: &str, tmp_ref: &str, name: &str) -> GuestCmd {
    git_cmd(
        "git_branch_plan",
        "set -e\ncd \"$A_repo\"\nwant=$(git rev-parse \"$A_ref^{commit}\")\nn=\"$A_name\"; i=1\nwhile :; do\n  git check-ref-format \"refs/heads/$n\" || { echo \"$n is not a branch name\" >&2; exit 3; }\n  if ! git show-ref --verify -q \"refs/heads/$n\"; then echo \"$n\"; echo new; exit 0; fi\n  if [ \"$(git rev-parse \"refs/heads/$n\")\" = \"$want\" ]; then echo \"$n\"; echo same; exit 0; fi\n  i=$((i+1)); n=\"$A_name-$i\"\n  [ $i -le 100 ] || { echo 'no free branch name' >&2; exit 3; }\ndone",
    )
    .arg("repo", repo)
    .arg("ref", tmp_ref)
    .arg("name", name)
}

/// Creates the branch `name` at the fetched commits. It fails if the branch exists: nothing
/// is merged, moved or checked out.
pub fn git_branch(repo: &str, tmp_ref: &str, name: &str) -> GuestCmd {
    git_cmd(
        "git_branch",
        "set -e\ncd \"$A_repo\"\ngit update-ref \"refs/heads/$A_name\" \"$A_ref^{commit}\" \"\"\ngit rev-parse --short \"refs/heads/$A_name\"",
    )
    .arg("repo", repo)
    .arg("ref", tmp_ref)
    .arg("name", name)
}

pub fn git_drop_ref(repo: &str, tmp_ref: &str) -> GuestCmd {
    git_cmd(
        "git_drop_ref",
        "cd \"$A_repo\" 2>/dev/null && git update-ref -d \"$A_ref\" 2>/dev/null; true",
    )
    .arg("repo", repo)
    .arg("ref", tmp_ref)
}

pub fn git_merge(repo: &str, tmp_ref: &str) -> GuestCmd {
    git_cmd(
        "git_merge",
        "cd \"$A_repo\"\nif git merge --ff-only -q \"$A_ref\" 2>/dev/null; then :; elif git -c user.name=Silo -c user.email=silo@localhost merge --no-edit -q \"$A_ref\"; then :; else git merge --abort 2>/dev/null; echo 'merge conflict' >&2; exit 3; fi\ngit update-ref -d \"$A_ref\"\ngit rev-parse --short HEAD",
    )
    .arg("repo", repo)
    .arg("ref", tmp_ref)
}

pub fn git_clone(repo: &str, bundle: &str) -> GuestCmd {
    git_cmd(
        "git_clone",
        "set -e\nmkdir -p -- \"$(dirname -- \"$A_repo\")\"\na=$(dirname -- \"$A_repo\"); while [ ! -d \"$a\" ]; do a=$(dirname -- \"$a\"); done\no=$(stat -c %u:%g -- \"$a\" 2>/dev/null || stat -f %u:%g -- \"$a\")\ncommand git clone -q \"$A_bundle\" \"$A_repo\"\nif [ \"$(id -u)\" = 0 ] && [ \"${o%%:*}\" != 0 ]; then chown -R \"$o\" \"$A_repo\"; fi\ncd \"$A_repo\"\ngit remote remove origin\ngit rev-parse --short HEAD",
    )
    .arg("repo", repo)
    .arg("bundle", bundle)
}

/// NUL-delimited paths that any commit in `old..new` touched, including ones a later commit
/// deleted (the whole history of `new` when `old` is empty). Blank entries may appear
/// between commits.
pub fn git_changed(repo: &str, old: &str, new: &str) -> GuestCmd {
    git_cmd(
        "git_changed",
        "cd \"$A_repo\" || exit 1\nif [ -z \"$A_old\" ]; then r=\"$A_new\"; else r=\"$A_old..$A_new\"; fi\ngit log -z --name-only --format= \"$r\" --",
    )
    .arg("repo", repo)
    .arg("old", old)
    .arg("new", new)
}

/// `blob main` for a repo-relative path: the blob at `base` (`-` if none) and the hash of
/// what the working tree holds (`-` if missing, `L:target` for a link).
pub fn git_file_state(repo: &str, rel: &str, base: &str) -> GuestCmd {
    git_cmd(
        "git_file_state",
        "cd \"$A_repo\" || exit 1\nb=-; if [ -n \"$A_base\" ]; then b=$(git rev-parse -q --verify \"$A_base:$A_rel\" 2>/dev/null || echo -); fi\nif [ -L \"$A_rel\" ]; then m=\"L:$(readlink -- \"$A_rel\")\"; elif [ -f \"$A_rel\" ]; then m=$(git hash-object -- \"$A_rel\"); else m=-; fi\necho \"$b $m\"",
    )
    .arg("repo", repo)
    .arg("rel", rel)
    .arg("base", base)
}

pub fn crontab_read(user: &str) -> GuestCmd {
    cmd(
        "crontab_read",
        "e=$(mktemp)\ncrontab -u \"$A_user\" -l 2>\"$e\" || { grep -q 'no crontab' \"$e\" || { cat \"$e\" >&2; rm -f \"$e\"; exit 1; }; }\nrm -f \"$e\"",
    )
    .arg("user", user)
}

pub fn crontab_write(user: &str, text: &str) -> GuestCmd {
    cmd("crontab_write", "crontab -u \"$A_user\" -")
        .arg("user", user)
        .stdin(text.as_bytes().to_vec())
}

/// Loads a LaunchAgent or daemon. A failure fails the keep.
pub fn agent_load(scope: &str, label: &str, path: &str) -> GuestCmd {
    cmd(
        "agent_load",
        "set -e\nif [ \"$A_scope\" = system ]; then d=system; else d=gui/$(stat -f %u /dev/console); fi\nlaunchctl bootout \"$d/$A_label\" 2>/dev/null || true\nlaunchctl bootstrap \"$d\" \"$A_path\"",
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

/// A recipe hook, run in `cwd` as the working account, or as the folder's owner when the
/// script starts as root.
pub fn hook(command: &str, cwd: &str) -> GuestCmd {
    let mut c = cmd(
        "hook",
        &format!("{AS_OWNER}cd \"$A_cwd\" && as_owner \"$A_cwd\" bash -lc \"$A_cmd\""),
    )
    .arg("cmd", command)
    .arg("cwd", cwd);
    c.timeout = Duration::from_secs(900);
    c
}
