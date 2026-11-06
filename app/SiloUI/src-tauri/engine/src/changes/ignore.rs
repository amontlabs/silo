//! The fixed ignore list and the credential path rules used by the change review.

/// Patterns for paths that are never reviewed (logs, caches, build output, files
/// rewritten at boot). Glob syntax: `*` matches any run of characters including `/`,
/// `?` one character, `[abc]` and `[!abc]` classes. A pattern without `/` is matched
/// against the file's base name; with `/` against the whole absolute path, and it also
/// matches everything below it (`/var/log` covers `/var/log/x/y`). The `*/` prefix stands
/// for any home directory (`/root`, `/home/<user>`, `/Users/<user>`).
pub const BUILTIN_IGNORE: &str = "\
# logs
*.log
*.log.*
/var/log
*/.npm/_logs

# shell / tool history and session noise
.bash_history
.zsh_history
.zsh_sessions
.viminfo
.lesshst
.python_history
.node_repl_history
.sqlite_history
.wget-hsts
.sudo_as_admin_successful
.zcompdump*
*.swp
*.swo
.DS_Store

# bytecode and build output
*.pyc
*/__pycache__
*/node_modules
*/target/debug
*/target/release

# caches and per-user state
*/.cache
*/.npm
*/.cargo/registry
*/.local/state
*/.local/share/Trash
*/.Trash
*/Library/Caches
*/Library/Saved Application State

# PulseAudio and the desktop session rewrite these on every login: the default output and
# input device per machine id, remembered volumes, the auth cookie
*/.config/pulse/*-default-sink
*/.config/pulse/*-default-source
*/.config/pulse/*-device-volumes.*
*/.config/pulse/*-stream-volumes.*
*/.config/pulse/*-card-database.*
*/.config/pulse/cookie
.ICEauthority
.Xauthority
*/.local/share/recently-used.xbel

# macOS background churn, from the files an idle macOS fork rewrites within minutes of its
# first start (preference caches, contacts and knowledge databases with their write-ahead
# files, system management state, Spotlight and Biome event stores)
*/Library/Preferences/ByHost
*/Library/Preferences/com.apple.*
*/Library/Preferences/.GlobalPreferences*
*/Library/Application Support/AddressBook
*/Library/Application Support/Knowledge
*/Library/Application Support/com.apple.RemoteManagementAgent
*/Library/Biome
*/Library/Daemon Containers
*/Library/Metadata
# the TV app's library databases, rewritten while it idles
*/Movies/TV/TV Library.tvlibrary
*/Library/Logs
*/Library/Group Containers/group.com.apple.*
*/Library/Containers/com.apple.*
*.db-shm
*.db-wal
*.sqlite-shm
*.sqlite-wal
*.abcddb-shm
*.abcddb-wal

# computer use: the driver's identity files (written even with telemetry off) and recordings,
# the guest helper's receipt, the desktop service's streamer receipt, package download and
# install lock, helper staging and the bridge's temporary files (macOS keeps /tmp at
# /private/tmp), and the macOS helper's receipt and screen recordings
/home/*/.cua-driver/.telemetry_id
/home/*/.cua-driver/.telemetry_identity.lock
/Users/*/.cua-driver/.telemetry_id
/Users/*/.cua-driver/.telemetry_identity.lock
/home/*/.local/share/cua-driver
/home/*/.config/cua-driver/recordings
/var/lib/silo-computer-use
/var/lib/silo-desktop/streamer.json
/var/lib/silo-desktop/selkies.deb
/var/lib/silo-desktop/install.lock
/tmp/silo-cu.*
/tmp/cua-driver
/tmp/cua-driver.*
/private/tmp/silo-cu.*
/private/tmp/cua-driver
/private/tmp/cua-driver.*
/Users/*/Library/Application Support/Silo/computer-use-receipt.json
/Users/*/Library/Application Support/Silo/Recordings

# agent session data
*/.claude/projects
.claude.json.backup*
.claude.json.corrupted*

# files rewritten by the system at boot or by tools
/etc/ld.so.cache
/etc/mtab
/etc/resolv.conf
/etc/hostname
/etc/hosts
/etc/passwd-
/etc/shadow-
/etc/group-
/etc/gshadow-
/etc/subuid-
/etc/subgid-
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgnoreList {
    patterns: Vec<String>,
}

impl IgnoreList {
    /// The built-in list ([`BUILTIN_IGNORE`]).
    pub fn builtin() -> Self {
        Self::parse(BUILTIN_IGNORE)
    }

    /// One pattern per line; blank lines and lines starting with `#` are skipped.
    pub fn parse(text: &str) -> Self {
        Self {
            patterns: text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(str::to_owned)
                .collect(),
        }
    }

    /// Adds the patterns of another list (for per-recipe additions).
    pub fn extend(&mut self, other: &IgnoreList) {
        self.patterns.extend(other.patterns.iter().cloned());
    }

    pub fn matches(&self, path: &str) -> bool {
        let base = path.rsplit('/').next().unwrap_or(path);
        self.patterns.iter().any(|pat| {
            if !pat.contains('/') {
                glob(pat, base)
            } else {
                glob(pat, path) || glob(&format!("{}/*", pat.trim_end_matches('/')), path)
            }
        })
    }
}

impl Default for IgnoreList {
    fn default() -> Self {
        Self::builtin()
    }
}

/// Case-sensitive shell-style match of the whole string.
pub fn glob(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        let step = if pi < p.len() {
            match p[pi] {
                '*' => {
                    star = Some((pi, ti));
                    pi += 1;
                    continue;
                }
                '?' => Some(pi + 1),
                '[' => match class_end(&p, pi) {
                    Some((end, neg, set)) => (class_has(&set, t[ti]) != neg).then_some(end),
                    // an unclosed bracket is an ordinary character, as in fnmatch
                    None => (t[ti] == '[').then_some(pi + 1),
                },
                c => (c == t[ti]).then_some(pi + 1),
            }
        } else {
            None
        };
        match step {
            Some(next) => {
                pi = next;
                ti += 1;
            }
            None => match star {
                Some((sp, st)) => {
                    pi = sp + 1;
                    ti = st + 1;
                    star = Some((sp, st + 1));
                }
                None => return false,
            },
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// For `[...]` at `p[start]`: (index after the class, negated, members). `None` when the
/// bracket is not closed, in which case it is an ordinary character.
fn class_end(p: &[char], start: usize) -> Option<(usize, bool, Vec<char>)> {
    let mut i = start + 1;
    // Only `!` negates; `[^a]` is the set of `^` and `a`, as in Python's fnmatch.
    let neg = matches!(p.get(i), Some('!'));
    if neg {
        i += 1;
    }
    let first = i;
    let mut set = Vec::new();
    while i < p.len() {
        if p[i] == ']' && i > first {
            return Some((i + 1, neg, set));
        }
        set.push(p[i]);
        i += 1;
    }
    None
}

fn class_has(set: &[char], c: char) -> bool {
    let mut i = 0;
    while i < set.len() {
        if i + 2 < set.len() && set[i + 1] == '-' {
            if set[i] <= c && c <= set[i + 2] {
                return true;
            }
            i += 3;
        } else {
            if set[i] == c {
                return true;
            }
            i += 1;
        }
    }
    false
}

/// Directories whose contents are credentials. A changed file below one of these is
/// reported under Not kept as one group, never as a settings or other change.
const CREDENTIAL_DIRS: &[&str] = &[
    "/.ssh/",
    "/.gnupg/",
    "/Library/Keychains/",
    // macOS permission grants (camera, files, automation), rewritten while idle: a change
    // is reported as not kept, never carried to another computer.
    "/Library/Application Support/com.apple.TCC/",
    // OpenClaw keeps provider tokens next to its config.
    "/.openclaw/credentials/",
    "/.config/gcloud/",
    "/.azure/",
    "/etc/ssl/private/",
    "/etc/letsencrypt/",
];

/// Exact absolute paths of system credential files.
const CREDENTIAL_PATHS: &[&str] = &[
    "/etc/shadow",
    "/etc/gshadow",
    "/etc/master.passwd",
    "/etc/sudoers",
    // The desktop's connection password.
    "/var/lib/silo-desktop/connection.json",
];

/// Glob patterns matched against the whole path (host keys and the like).
const CREDENTIAL_GLOBS: &[&str] = &[
    "/etc/ssh/ssh_host_*_key",
    "*/ssh_host_*_key",
    "*_rsa_key",
    "*_ed25519_key",
];

/// Exact file names (any directory).
const CREDENTIAL_NAMES: &[&str] = &[
    ".netrc",
    ".git-credentials",
    ".pypirc",
    ".npmrc",
    // ssh private keys that were copied out of ~/.ssh
    "id_rsa",
    "id_dsa",
    "id_ecdsa",
    "id_ed25519",
    "id_ecdsa_sk",
    "id_ed25519_sk",
];

/// Path suffixes of single credential files.
const CREDENTIAL_SUFFIXES: &[&str] = &[
    "/gh/hosts.yml",
    "/.docker/config.json",
    "/.aws/credentials",
    "/.kube/config",
];

/// Base-name extensions of key material, wherever it sits.
const CREDENTIAL_EXTENSIONS: &[&str] = &[
    ".pem",
    ".key",
    ".p12",
    ".pfx",
    ".p8",
    ".ppk",
    ".jks",
    ".keystore",
    ".keychain",
    ".keychain-db",
];

/// Trees of installed packages in which key-material extensions name the package's own
/// files; the exemption needs ownership evidence (an `OWN` record) for the path.
const OWNED_TREES: &[&str] = &["/usr/", "/opt/homebrew/"];

/// Installed-package trees where the extension rule does not apply.
const PACKAGE_TREES: &[&str] = &[
    "/site-packages/",
    "/dist-packages/",
    "/node_modules/",
    "/share/ca-certificates/",
    "/etc/ssl/certs/",
];

/// The Not kept group a credential path belongs to: the directory (ending in `/`) for
/// directory rules, the path itself for single files. `None` when the path is not a
/// credential.
pub fn credential_group(path: &str) -> Option<String> {
    credential_group_owned(path, false)
}

/// Like [`credential_group`]; `owned` says a package manager owns the path (`OWN`), which
/// exempts key-material extensions under `/usr/` and `/opt/homebrew/` (bundled CA files).
pub fn credential_group_owned(path: &str, owned: bool) -> Option<String> {
    if CREDENTIAL_PATHS.contains(&path) || CREDENTIAL_GLOBS.iter().any(|g| glob(g, path)) {
        return Some(path.to_owned());
    }
    for marker in CREDENTIAL_DIRS {
        if let Some(i) = path.find(marker) {
            return Some(path[..i + marker.len()].to_owned());
        }
    }
    let base = path.rsplit('/').next().unwrap_or(path);
    // Key-material extensions also name bundled CA certificates inside installed packages
    // (certifi's cacert.pem); those are the package's files, not credentials.
    let extension_rule = !PACKAGE_TREES.iter().any(|t| path.contains(t))
        && !(owned && OWNED_TREES.iter().any(|t| path.starts_with(t)))
        && CREDENTIAL_EXTENSIONS.iter().any(|e| base.ends_with(e));
    if CREDENTIAL_NAMES.contains(&base)
        || CREDENTIAL_SUFFIXES.iter().any(|s| path.ends_with(s))
        || extension_rule
    {
        return Some(path.to_owned());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tv_library_is_background_churn() {
        let list = IgnoreList::builtin();
        assert!(list.matches("/Users/a/Movies/TV/TV Library.tvlibrary/library.tvdb"));
        assert!(!list.matches("/Users/a/Movies/film.mov"));
    }

    #[test]
    fn computer_use_state_is_not_a_change() {
        let list = IgnoreList::builtin();
        for path in [
            "/var/lib/silo-computer-use/receipt.json",
            "/var/lib/silo-desktop/streamer.json",
            "/private/tmp/silo-cu.AbCd1234/helper",
            "/var/log/silo-computer-use.log",
            "/home/silo/.cache/cua-driver/shots/1.png",
            "/home/silo/.local/share/cua-driver/trajectories/a/step.json",
            "/home/silo/.cua-driver/.telemetry_id",
            "/Users/silo/.cua-driver/.telemetry_identity.lock",
            "/var/lib/silo-desktop/selkies.deb",
            "/var/lib/silo-desktop/install.lock",
            "/tmp/cua-driver.1234",
            "/home/silo/.Xauthority",
            "/tmp/silo-cu.AbCd1234/helper",
            "/Users/silo/Library/Application Support/Silo/computer-use-receipt.json",
            "/Users/silo/Library/Application Support/Silo/Recordings/guest-screen.mov",
            "/Users/silo/Library/Logs/silo-computer-use.log",
            "/Users/silo/Library/Group Containers/group.com.apple.replayd/ScreenCaptureApprovals.plist",
        ] {
            assert!(list.matches(path), "{path} should be ignored");
        }
        assert!(!list.matches("/home/silo/project/src/main.rs"));
        assert!(!list.matches("/home/silo/.config/Code/settings.json"));
        // Settings a person may have changed stay reviewable.
        assert!(!list.matches("/var/lib/silo-desktop/config.json"));
        assert!(!list.matches("/home/silo/.cua-driver/config.json"));
        assert!(!list.matches("/tmp/cua-driver-notes.txt"));
        // The desktop password is a credential, reported under Not kept.
        assert!(credential_group("/var/lib/silo-desktop/connection.json").is_some());
        assert!(!list.matches("/home/silo/.config/xfce4/xfconf/xfce-perchannel-xml/displays.xml"));
        assert!(!list.matches("/Users/silo/Library/Application Support/Silo/notes.txt"));
    }

    #[test]
    fn glob_basics() {
        assert!(glob("*.log", "a.log"));
        assert!(glob("*.log.*", "a.log.1"));
        assert!(!glob("*.log", "a.logx"));
        assert!(glob("*/node_modules", "/root/app/node_modules"));
        assert!(glob("a?c", "abc"));
        assert!(glob("[a-c]x", "bx"));
        assert!(!glob("[!a-c]x", "bx"));
        assert!(glob(".zcompdump*", ".zcompdump-host-5.9"));
    }

    #[test]
    fn ignore_matches_names_and_subtrees() {
        let l = IgnoreList::builtin();
        assert!(l.matches("/root/.bash_history"));
        assert!(l.matches("/var/log/apt/history.log"));
        assert!(l.matches("/root/app/node_modules/x/y.js"));
        assert!(l.matches("/Users/a/Library/Caches/com.x/db"));
        assert!(!l.matches("/root/app/src/main.rs"));
        assert!(!l.matches("/etc/postgresql/16/main/postgresql.conf"));
    }

    #[test]
    fn glob_matches_fnmatch_brackets() {
        assert!(glob("a[b", "a[b"));
        assert!(!glob("a[b", "ab"));
        assert!(glob("[!a]x", "bx"));
        assert!(!glob("[!a]x", "ax"));
        assert!(glob("[^a]x", "^x"));
        assert!(glob("[^a]x", "ax"));
        assert!(!glob("[^a]x", "bx"));
        assert!(glob("[]a]x", "]x"));
    }

    #[test]
    fn system_and_host_keys_are_credentials() {
        for p in [
            "/etc/shadow",
            "/etc/gshadow",
            "/etc/ssh/ssh_host_ed25519_key",
            "/etc/ssh/ssh_host_rsa_key",
            "/srv/app/tls.key",
            "/usr/local/share/app/private.pem",
        ] {
            assert!(credential_group(p).is_some(), "{p}");
        }
        assert_eq!(credential_group("/etc/ssh/ssh_host_ed25519_key.pub"), None);
        // bundled CA files are exempt only with package ownership
        assert!(credential_group_owned("/usr/share/x/cacert.pem", true).is_none());
        assert!(credential_group_owned("/usr/share/x/cacert.pem", false).is_some());
    }

    #[test]
    fn parse_skips_comments() {
        let l = IgnoreList::parse("# c\n\n*.tmp\n");
        assert!(l.matches("/x/a.tmp"));
        assert!(!l.matches("/x/a.log"));
    }

    #[test]
    fn credential_rules() {
        for (p, g) in [
            (
                "/root/.openclaw/credentials/pdf-tools.json",
                "/root/.openclaw/credentials/",
            ),
            ("/Users/a/.ssh/id_ed25519", "/Users/a/.ssh/"),
            ("/Users/a/.ssh/known_hosts", "/Users/a/.ssh/"),
            (
                "/Users/a/Library/Keychains/login.keychain-db",
                "/Users/a/Library/Keychains/",
            ),
            ("/root/.aws/credentials", "/root/.aws/credentials"),
            ("/root/.config/gh/hosts.yml", "/root/.config/gh/hosts.yml"),
            ("/srv/certs/site.pem", "/srv/certs/site.pem"),
            ("/root/.netrc", "/root/.netrc"),
        ] {
            assert_eq!(credential_group(p).as_deref(), Some(g), "{p}");
        }
        assert_eq!(credential_group("/root/.openclaw/openclaw.json"), None);
        assert_eq!(credential_group("/root/.aws/config"), None);
        assert_eq!(credential_group("/root/app/src/lib.rs"), None);
    }
}
