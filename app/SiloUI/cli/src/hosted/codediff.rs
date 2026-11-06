//! The Code bucket's changes as unified diff text, for the code lens. The diff is read inside
//! the computer with the same hardening the state collector uses for repositories, then cut
//! down here: files that look like credentials or environment files are dropped (also when
//! they are the source of a rename), binary changes are dropped, token-shaped strings are
//! redacted, and the size is capped. Untracked and ignored files appear by name only (in the
//! document), never by content.

use regex::{Captures, Regex};
use serde_json::Value;

use super::document::hidden;
use crate::{
    engine::Engine,
    ops::{self, ExecArgs},
};

/// The most lines kept per file and in all.
pub const MAX_FILE_LINES: usize = 300;
pub const MAX_TOTAL_BYTES: usize = 120 * 1024;
const MARKER: &str = "[truncated]";
const REDACTED: &str = "[redacted]";
/// Git's empty tree: the base of a repository that had no commits when the fork was made.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// Path patterns git leaves out of the diff. Every name `document::hidden` treats as a
/// credential has a pattern here.
pub const EXCLUDES: &[&str] = &[
    "**/.env",
    "**/.env.*",
    "**/*.env",
    "**/*.pem",
    "**/*.key",
    "**/*.p12",
    "**/*.pfx",
    "**/*.p8",
    "**/*.ppk",
    "**/*.jks",
    "**/*.keystore",
    "**/*.keychain",
    "**/*.keychain-db",
    "**/*.tfvars",
    "**/*.tfstate*",
    "**/*.kdbx",
    "**/*.ovpn",
    "**/*-sa.json",
    "**/firebase-adminsdk*",
    "**/id_rsa*",
    "**/id_dsa*",
    "**/id_ecdsa*",
    "**/id_ed25519*",
    "**/ssh_host_*_key",
    "**/*_rsa_key",
    "**/*_ed25519_key",
    "**/.ssh/**",
    "**/.aws/**",
    "**/.gnupg/**",
    "**/.azure/**",
    "**/.config/gcloud/**",
    "**/.config/gh/hosts.yml",
    "**/.openclaw/credentials/**",
    "**/.netrc",
    "**/.npmrc",
    "**/.pypirc",
    "**/.pgpass",
    "**/.htpasswd",
    "**/.vault-token",
    "**/.dockercfg",
    "**/.git-credentials",
    "**/.docker/config.json",
    "**/.kube/config",
    "**/kubeconfig",
    "**/credentials*",
    "**/secrets.yaml",
    "**/secrets.json",
];

/// Runs in bash inside the computer: `$1` repo, `$2` base (or empty), `$3` `names` or `diff`,
/// then extra pathspecs. The git environment, configuration switches and owner switch are the
/// state collector's (`changes/collect.sh`).
const HARDENING: &str = r#"
have() { command -v "$1" >/dev/null 2>&1; }
GIT_ENV=(env -i PATH="$PATH" HOME=/nonexistent XDG_CONFIG_HOME=/nonexistent LC_ALL=C
  GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_TERMINAL_PROMPT=0 GIT_OPTIONAL_LOCKS=0
  GIT_ASKPASS=/bin/false GIT_ATTR_NOSYSTEM=1)
GPRE=()
repo_prefix() {
  local ids uid gid name
  GPRE=()
  [ "$(id -u)" = 0 ] || return 0
  if stat --version >/dev/null 2>&1; then ids=$(stat -c '%u %g' "$1/.git" 2>/dev/null)
  else ids=$(stat -f '%u %g' "$1/.git" 2>/dev/null); fi
  uid=${ids% *}; gid=${ids#* }
  case "$uid" in ''|*[!0-9]*) return 1;; esac
  case "$gid" in ''|*[!0-9]*) return 1;; esac
  [ "$uid" = 0 ] && return 0
  if have setpriv; then GPRE=(setpriv --reuid="$uid" --regid="$gid" --clear-groups); return 0; fi
  name=$(id -nu "$uid" 2>/dev/null) || return 1
  if have runuser; then GPRE=(runuser -u "$name" --); return 0; fi
  return 1
}
R=$1; BASE=$2; MODE=$3; shift 3
repo_prefix "$R" || exit 0
gg() { "${GPRE[@]}" "${GIT_ENV[@]}" git --no-optional-locks -c core.fsmonitor=false -c core.untrackedCache=false \
  -c core.hooksPath=/dev/null -c core.pager=cat -c core.quotePath=false -c core.attributesFile=/dev/null \
  -c diff.external= -c safe.directory='*' -C "$R" "$@" 2>/dev/null; }
OPTS=(-M --ignore-submodules=all)
if [ "$MODE" = names ]; then
  run() {
    [ -n "$BASE" ] && gg diff "${OPTS[@]}" -z --name-status --end-of-options "$BASE" HEAD --
    gg diff "${OPTS[@]}" -z --name-status --end-of-options HEAD --
  }
else
  OPTS+=(--no-color --no-ext-diff --no-textconv --src-prefix=a/ --dst-prefix=b/ -U3)
  run() {
    [ -n "$BASE" ] && gg diff "${OPTS[@]}" --end-of-options "$BASE" HEAD -- . "$@"
    gg diff "${OPTS[@]}" --end-of-options HEAD -- . "$@"
  }
fi
run "$@" | head -c 2000000
"#;

/// The pathspecs of the excluded names.
fn exclude_specs() -> Vec<String> {
    EXCLUDES
        .iter()
        .map(|pattern| format!(":(exclude,glob){pattern}"))
        .collect()
}

/// A commit id as `git` prints it.
fn valid_commit(text: &str) -> bool {
    (7..=64).contains(&text.len()) && text.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
}

/// What the diff starts from, or an empty string when only uncommitted changes are read.
fn base_of(repo: &Value) -> String {
    if !repo["new"].is_null() {
        return String::new();
    }
    match repo["commits"][0].as_str() {
        Some("-") => EMPTY_TREE.to_owned(),
        Some(old) if valid_commit(old) => old.to_owned(),
        _ => String::new(),
    }
}

/// The destinations of renames and copies whose source is a hidden file.
pub fn hidden_destinations(repo: &str, names: &str) -> Vec<String> {
    let mut fields = names.split('\0');
    let mut out = Vec::new();
    while let Some(status) = fields.next() {
        if status.is_empty() {
            continue;
        }
        let first = fields.next().unwrap_or_default();
        if status.starts_with('R') || status.starts_with('C') {
            let second = fields.next().unwrap_or_default();
            if hidden(repo, first) {
                out.push(format!(":(exclude,literal){second}"));
            }
        }
    }
    out
}

async fn run_in(
    engine: &Engine,
    computer: &str,
    repo: &str,
    base: &str,
    mode: &str,
    extra: &[String],
) -> Option<String> {
    let mut argv: Vec<String> = ["bash", "-c", HARDENING, "bash", repo, base, mode]
        .map(str::to_owned)
        .to_vec();
    argv.extend(extra.iter().cloned());
    let args = ExecArgs {
        computer: computer.to_owned(),
        argv,
        cwd: None,
        timeout_secs: 60,
    };
    let result = ops::exec_raw(engine, &args).await.ok()?;
    Some(String::from_utf8_lossy(&result.stdout).into_owned())
}

/// Runs the diff in each changed repo of the computer and returns the cut-down text.
pub async fn read(engine: &Engine, computer: &str, document: &Value, home: &str) -> String {
    let mut out = String::new();
    for repo in document["code"].as_array().into_iter().flatten() {
        let Some(path) = repo["repo"].as_str() else {
            continue;
        };
        if repo["removed"] == true || hidden("", &format!("{}/", path.trim_end_matches('/'))) {
            continue;
        }
        let base = base_of(repo);
        let names = run_in(engine, computer, path, &base, "names", &[])
            .await
            .unwrap_or_default();
        let mut extra = exclude_specs();
        extra.extend(hidden_destinations(path, &names));
        let Some(raw) = run_in(engine, computer, path, &base, "diff", &extra).await else {
            continue;
        };
        let label = path.replacen(&format!("{home}/"), "~/", 1);
        let cut = process(path, &raw);
        if !cut.is_empty() {
            out.push_str(&format!("# repo: {label}\n{cut}"));
        }
        if out.len() > MAX_TOTAL_BYTES {
            break;
        }
    }
    cap_total(out)
}

fn cap_total(mut text: String) -> String {
    if text.len() > MAX_TOTAL_BYTES {
        let mut end = MAX_TOTAL_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str(&format!("\n{MARKER}\n"));
    }
    text
}

/// The paths a file's header lines name (before its first hunk).
fn header_paths(header: &[&str]) -> Vec<String> {
    let mut paths = Vec::new();
    for line in header {
        let named = line
            .strip_prefix("--- a/")
            .or_else(|| line.strip_prefix("+++ b/"))
            .or_else(|| line.strip_prefix("rename from "))
            .or_else(|| line.strip_prefix("rename to "))
            .or_else(|| line.strip_prefix("copy from "))
            .or_else(|| line.strip_prefix("copy to "));
        if let Some(path) = named {
            paths.push(path.to_owned());
        } else if line.starts_with("--- \"") || line.starts_with("+++ \"") {
            paths.push(line[4..].to_owned());
        }
    }
    if paths.is_empty() {
        // A change with no hunk (a mode change, an empty file): the names on the first line.
        let rest = header[0].strip_prefix("diff --git a/").unwrap_or_default();
        match rest.split_once(" b/") {
            Some((a, b)) => paths.extend([a.to_owned(), b.to_owned()]),
            None => paths.push(rest.to_owned()),
        }
    }
    paths
}

/// One repo's raw diff text, file by file, cut down. `repo` is its absolute path.
pub fn process(repo: &str, raw: &str) -> String {
    let mut files: Vec<Vec<&str>> = Vec::new();
    for line in raw.lines() {
        if line.starts_with("diff --git ") {
            files.push(Vec::new());
        }
        if let Some(file) = files.last_mut() {
            file.push(line);
        }
    }
    let mut out = String::new();
    for file in files {
        let hunk = file
            .iter()
            .position(|l| l.starts_with("@@ "))
            .unwrap_or(file.len());
        let (header, body) = file.split_at(hunk);
        let binary = header
            .iter()
            .any(|l| l.starts_with("Binary files ") || l.starts_with("GIT binary patch"))
            || body.iter().any(|l| l.starts_with("GIT binary patch"));
        let paths = header_paths(header);
        if binary
            || paths
                .iter()
                .any(|p| p.starts_with('"') || p.is_empty() || hidden(repo, p))
        {
            continue;
        }
        for line in header {
            out.push_str(line);
            out.push('\n');
        }
        let mut in_key = false;
        for (count, line) in body.iter().enumerate() {
            if count >= MAX_FILE_LINES {
                out.push_str(&format!("{MARKER} ({} more lines)\n", body.len() - count));
                break;
            }
            if line.contains("PRIVATE KEY") && (line.contains("BEGIN") || line.contains("END")) {
                if line.contains("BEGIN") {
                    in_key = true;
                    out.push_str(&format!("+{REDACTED} private key\n"));
                } else {
                    in_key = false;
                }
                continue;
            }
            if in_key {
                continue;
            }
            out.push_str(&redact(line));
            out.push('\n');
        }
    }
    out
}

fn entropy(text: &str) -> f64 {
    let mut counts = std::collections::HashMap::new();
    for c in text.chars() {
        *counts.entry(c).or_insert(0usize) += 1;
    }
    let len = text.chars().count() as f64;
    counts
        .values()
        .map(|n| {
            let p = *n as f64 / len;
            -p * p.log2()
        })
        .sum()
}

/// A long run of token characters that looks random rather than like a name or a path.
fn random_looking(text: &str) -> bool {
    let letters = text.chars().any(|c| c.is_ascii_alphabetic());
    let numbers = text.chars().any(|c| c.is_ascii_digit());
    let hash = matches!(text.len(), 40 | 64) && text.chars().all(|c| c.is_ascii_hexdigit());
    let path = text.matches('/').count() >= 2;
    letters && numbers && !hash && !path && entropy(text) >= 3.5
}

struct Rules {
    simple: Vec<(Regex, &'static str)>,
    keyword: Regex,
    long: Regex,
}

fn rules() -> &'static Rules {
    use std::sync::OnceLock;
    static RULES: OnceLock<Rules> = OnceLock::new();
    RULES.get_or_init(|| {
        let make = |expr: &str, with: &'static str| (Regex::new(expr).unwrap(), with);
        Rules {
            simple: vec![
                make(r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b", REDACTED),
                make(r"\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{20,}", REDACTED),
                make(r"\bgithub_pat_[A-Za-z0-9_]{20,}", REDACTED),
                make(r"\bglpat-[A-Za-z0-9_-]{16,}", REDACTED),
                make(r"\bsk-[A-Za-z0-9_-]{16,}", REDACTED),
                make(r"\bxox[abpsr]-[A-Za-z0-9-]{10,}", REDACTED),
                make(r"\b(?:rk|pk|sk)_(?:live|test)_[A-Za-z0-9]{10,}", REDACTED),
                make(r"\bwhsec_[A-Za-z0-9]{10,}", REDACTED),
                make(r"([A-Za-z][A-Za-z0-9+.-]*://)[^/\s@]+@", "${1}[redacted]@"),
                make(r"(?i)\b(bearer|basic)\s+[A-Za-z0-9._~+/=-]{8,}", "${1} [redacted]"),
            ],
            keyword: Regex::new(
                r#"(?i)\b([A-Za-z0-9_]*(?:secret|token|passw(?:or)?d|pwd|api[_-]?key|private[_-]?key|access[_-]?key|auth|credential|dsn)[A-Za-z0-9_]*["']?\s*[:=]\s*)("[^"]{6,}"|'[^']{6,}'|[^\s"',;#)\]}]{8,})"#,
            )
            .unwrap(),
            long: Regex::new(r"[A-Za-z0-9+/_=-]{32,}").unwrap(),
        }
    })
}

/// A value that stands for another one (`$VAR`, `{{x}}`, `<name>`), not a literal.
fn placeholder(value: &str) -> bool {
    let inner = value.trim_matches(|c| c == '"' || c == '\'');
    inner.starts_with('$')
        || inner.starts_with("{{")
        || inner.starts_with('<')
        || inner.starts_with('%')
}

fn redact_keyword(caps: &Captures) -> String {
    let (key, value) = (&caps[1], &caps[2]);
    let quoted = value.starts_with('"') || value.starts_with('\'');
    let literal = if quoted {
        !placeholder(value)
    } else {
        !placeholder(value)
            && (value.chars().any(|c| c.is_ascii_digit()) || value.chars().count() >= 20)
    };
    if !literal {
        return caps[0].to_owned();
    }
    let quote = if quoted { &value[..1] } else { "" };
    format!("{key}{quote}{REDACTED}{quote}")
}

/// `line` with token-shaped strings replaced.
pub fn redact(line: &str) -> String {
    let rules = rules();
    let mut text = line.to_owned();
    for (pattern, with) in &rules.simple {
        text = pattern.replace_all(&text, *with).into_owned();
    }
    text = rules
        .keyword
        .replace_all(&text, redact_keyword)
        .into_owned();
    rules
        .long
        .replace_all(&text, |caps: &Captures| {
            if random_looking(&caps[0]) {
                REDACTED.to_owned()
            } else {
                caps[0].to_owned()
            }
        })
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_token_shapes() {
        for secret in [
            "AKIAABCDEFGHIJKLMNOP",
            "ghp_abcdefghijklmnopqrstuvwxyz0123",
            "github_pat_abcdefghijklmnopqrstuv",
            "glpat-abcdefghijklmnopqrstu",
            "sk-abcdefghijklmnopqrstuv",
            "xoxb-1234567890-abcdef",
            "rk_live_abcdefghijklmnop",
            "pk_test_abcdefghijklmnop",
            "whsec_abcdefghijklmnop",
            "aGVsbG8gd29ybGQgdGhpcyBpcyBhIHRva2Vu0123456789",
        ] {
            let out = redact(&format!("+use = \"{secret}\""));
            assert!(!out.contains(secret), "{out}");
        }
        for (line, secret) in [
            (
                "+url = https://user:hunter2pass@db.example.com/x",
                "hunter2pass",
            ),
            ("+curl https://abcdef123456@example.com", "abcdef123456"),
            ("+Authorization: Bearer abc123def456", "abc123def456"),
            ("+password = hunter2hunter2", "hunter2hunter2"),
            ("+private_key: \"abcdef\"", "abcdef"),
            ("+AWS_ACCESS_KEY=abcd1234efgh", "abcd1234efgh"),
            ("+pwd: 'hunter22'", "hunter22"),
            ("+DATABASE_DSN=postgres0123456789", "postgres0123456789"),
        ] {
            let out = redact(line);
            assert!(!out.contains(secret), "{line} -> {out}");
        }
    }

    #[test]
    fn leaves_code_that_only_looks_like_a_secret() {
        for line in [
            "+let total = count + 1;",
            "+    token: String,",
            "+    token: Option<String>,",
            "+    auth_header: HeaderValue,",
            "+password = $PASSWORD",
            "+secret: \"${SECRET}\"",
            "+import some_very_long_module_name_that_goes_on_and_on",
            "+path = src/components/very/long/directory/name/file",
            "+commit 3b18e512dba79e4c8300dd08aeb37f8e728b8dad",
            "+a-very-long-kebab-case-identifier-with-many-words",
        ] {
            assert_eq!(redact(line), line);
        }
    }

    const RAW: &str = "diff --git a/src/app.rs b/src/app.rs\n--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1 +1 @@\n-old\n+new\n\
diff --git a/.env b/.env\n--- a/.env\n+++ b/.env\n@@ -1 +1 @@\n+TOKEN=abc\n\
diff --git a/deploy.key b/deploy.key\n+++ b/deploy.key\n@@ -1 +1 @@\n+x\n\
diff --git a/logo.png b/logo.png\nBinary files a/logo.png and b/logo.png differ\n\
diff --git a/notes.txt b/notes.txt\n--- a/notes.txt\n+++ b/notes.txt\n@@ -1 +1 @@\n+-----BEGIN PGP PRIVATE KEY BLOCK-----\n+MIIEabc\n+-----END PGP PRIVATE KEY BLOCK-----\n+after\n\
diff --git a/other.txt b/other.txt\n--- a/other.txt\n+++ b/other.txt\n@@ -1 +1 @@\n+MIIEstill\n+kept\n";

    #[test]
    fn drops_credential_files_binaries_and_key_blocks() {
        let out = process("/home/dev/app", RAW);
        assert!(out.contains("+new") && out.contains("+after") && out.contains("+kept"));
        for gone in [
            "TOKEN=abc",
            "deploy.key",
            "Binary files",
            "MIIEabc",
            "logo.png",
            ".env",
        ] {
            assert!(!out.contains(gone), "{gone} in {out}");
        }
        // A key block that never ends does not swallow the next file.
        assert!(out.contains("MIIEstill"));
        assert!(out.contains("private key"));
    }

    #[test]
    fn content_that_looks_like_a_header_is_redacted_not_trusted() {
        let raw = "diff --git a/s.sql b/s.sql\n--- a/s.sql\n+++ b/s.sql\n@@ -1,2 +1,2 @@\n--- a/x\n+++ b/api_key = \"abcd1234efgh5678\"\n";
        let out = process("/r", raw);
        assert!(!out.contains("abcd1234efgh5678"), "{out}");
    }

    #[test]
    fn paths_come_from_the_header_lines() {
        // A file name with " b/" in it cannot fool the check on the first line.
        let raw = "diff --git a/x b/.env b/ok.txt\n--- a/x b/.env\n+++ b/ok.txt\n@@ -1 +1 @@\n+v\n";
        assert_eq!(process("/r", raw), "");
        let rename = "diff --git a/new.txt b/new.txt\nsimilarity index 90%\nrename from id_rsa\nrename to new.txt\n@@ -1 +1 @@\n+v\n";
        assert_eq!(process("/r", rename), "");
        let quoted = "diff --git \"a/x\\ty\" \"b/x\\ty\"\n--- \"a/x\\ty\"\n+++ \"b/x\\ty\"\n@@ -1 +1 @@\n+v\n";
        assert_eq!(process("/r", quoted), "");
    }

    #[test]
    fn renames_from_hidden_files_hide_the_destination() {
        let names =
            "R100\0id_rsa\0notes/copy.txt\0M\0src/a.rs\0C90\0.env\0example.txt\0R100\0a.rs\0b.rs\0";
        assert_eq!(
            hidden_destinations("/r", names),
            vec![
                ":(exclude,literal)notes/copy.txt".to_owned(),
                ":(exclude,literal)example.txt".to_owned()
            ]
        );
    }

    #[test]
    fn caps_each_file_and_all_of_it() {
        let mut raw = String::from(
            "diff --git a/big.txt b/big.txt\n--- a/big.txt\n+++ b/big.txt\n@@ -1 +1 @@\n",
        );
        for i in 0..1000 {
            raw.push_str(&format!("+line {i}\n"));
        }
        let out = process("/r", &raw);
        assert!(out.lines().count() <= MAX_FILE_LINES + 4);
        assert!(out.contains(MARKER));
        let capped = cap_total("x".repeat(MAX_TOTAL_BYTES * 2));
        assert!(capped.len() < MAX_TOTAL_BYTES + 20 && capped.contains(MARKER));
    }

    #[test]
    fn the_base_is_checked_and_an_unborn_repo_starts_from_the_empty_tree() {
        use serde_json::json;
        assert_eq!(base_of(&json!({"commits": ["abc1234", "def"]})), "abc1234");
        assert_eq!(base_of(&json!({"commits": ["-", "def"]})), EMPTY_TREE);
        for bad in ["--output=/tmp/x", "ABCDEF1", "abc", "abc1234;rm", ""] {
            assert_eq!(base_of(&json!({"commits": [bad, "def"]})), "", "{bad}");
        }
        assert_eq!(
            base_of(&json!({"new": ["h", "main"], "commits": ["abc1234", "d"]})),
            ""
        );
    }

    #[test]
    fn the_script_is_hardened_and_every_hidden_name_is_excluded() {
        for needle in [
            "GIT_CONFIG_NOSYSTEM=1",
            "GIT_CONFIG_GLOBAL=/dev/null",
            "core.fsmonitor=false",
            "core.hooksPath=/dev/null",
            "core.attributesFile=/dev/null",
            "safe.directory='*'",
            "repo_prefix",
            "--no-textconv",
            "--no-ext-diff",
            "--ignore-submodules=all",
            "--src-prefix=a/ --dst-prefix=b/",
            "core.quotePath=false",
            "--end-of-options",
        ] {
            assert!(HARDENING.contains(needle), "{needle}");
        }
        for name in [
            "x/.env",
            "x/.env.local",
            "a.pem",
            "a.key",
            "a.tfvars",
            "a.tfstate",
            "a.tfstate.backup",
            ".pgpass",
            ".htpasswd",
            ".vault-token",
            ".dockercfg",
            "kubeconfig",
            "a.kdbx",
            "a.ovpn",
            "gcp-sa.json",
            "firebase-adminsdk-x.json",
            "id_rsa",
            "id_ed25519.pub",
            "credentials.json",
        ] {
            assert!(hidden("/r", name), "{name} is not hidden");
            let base = name.rsplit('/').next().unwrap();
            let covered = EXCLUDES.iter().any(|pattern| {
                let tail = pattern.trim_start_matches("**/");
                match tail.split_once('*') {
                    None => base == tail,
                    Some((head, rest)) => {
                        base.starts_with(head)
                            && (rest.is_empty() || base.contains(rest.trim_end_matches('*')))
                    }
                }
            });
            assert!(covered, "{name} has no exclude pattern");
        }
    }
}
