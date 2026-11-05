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
                '[' => class_end(&p, pi).and_then(|(end, neg, set)| {
                    (class_has(&set, t[ti]) != neg).then_some(end)
                }),
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
    let neg = matches!(p.get(i), Some('!') | Some('^'));
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
    // OpenClaw keeps provider tokens next to its config.
    "/.openclaw/credentials/",
    "/.config/gcloud/",
    "/.azure/",
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
];

/// Path suffixes of single credential files.
const CREDENTIAL_SUFFIXES: &[&str] = &[
    "/gh/hosts.yml",
    "/.docker/config.json",
    "/.aws/credentials",
    "/.kube/config",
];

/// Base-name extensions of key material, wherever it sits.
const CREDENTIAL_EXTENSIONS: &[&str] = &[".pem", ".p12", ".pfx", ".keychain", ".keychain-db"];

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
    for marker in CREDENTIAL_DIRS {
        if let Some(i) = path.find(marker) {
            return Some(path[..i + marker.len()].to_owned());
        }
    }
    let base = path.rsplit('/').next().unwrap_or(path);
    // Key-material extensions also name bundled CA certificates inside installed packages
    // (certifi's cacert.pem); those are the package's files, not credentials.
    let extension_rule = !PACKAGE_TREES.iter().any(|t| path.contains(t))
        && !path.starts_with("/usr/")
        && !path.starts_with("/opt/homebrew/")
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
            (
                "/root/.config/gh/hosts.yml",
                "/root/.config/gh/hosts.yml",
            ),
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
