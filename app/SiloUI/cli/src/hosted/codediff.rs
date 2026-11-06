//! The Code bucket's changes as unified diff text, for the code lens. The diff is read inside
//! the computer, then cut down here: files that look like credentials or environment files
//! are dropped, binary changes are dropped, token-shaped strings are redacted, and the size
//! is capped. Untracked and gitignored files appear by name only (in the document), never
//! by content.

use regex::Regex;
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

/// Path patterns git leaves out of the diff before it is read.
const EXCLUDES: &[&str] = &[
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
    "**/*.tfvars",
    "**/id_rsa*",
    "**/id_dsa*",
    "**/id_ecdsa*",
    "**/id_ed25519*",
    "**/.ssh/**",
    "**/.aws/**",
    "**/.gnupg/**",
    "**/.netrc",
    "**/.npmrc",
    "**/.pypirc",
    "**/.git-credentials",
    "**/credentials*",
    "**/.kube/config",
    "**/.docker/config.json",
];

fn script() -> String {
    let specs: String = EXCLUDES
        .iter()
        .map(|pattern| format!(" ':(exclude,glob){pattern}'"))
        .collect();
    let git =
        "git -c core.pager=cat -c diff.external= diff --no-color --no-ext-diff --no-textconv -U3";
    format!(
        "cd \"$1\" || exit 0; if [ -n \"$2\" ]; then {git} \"$2..HEAD\" -- .{specs}; fi; \
         {git} HEAD -- .{specs}"
    )
}

/// Runs the diff in each changed repo of the computer and returns the cut-down text.
pub async fn read(engine: &Engine, computer: &str, document: &Value, home: &str) -> String {
    let mut out = String::new();
    for repo in document["code"].as_array().into_iter().flatten() {
        let Some(path) = repo["repo"].as_str() else {
            continue;
        };
        if repo["removed"] == true {
            continue;
        }
        let old = if repo["new"].is_null() {
            repo["commits"][0].as_str().unwrap_or_default()
        } else {
            ""
        };
        let args = ExecArgs {
            computer: computer.to_owned(),
            argv: [
                "sh",
                "-c",
                &format!("{} | head -c 2000000", script()),
                "sh",
                path,
                old,
            ]
            .map(str::to_owned)
            .to_vec(),
            cwd: None,
            timeout_secs: 60,
        };
        let Ok(result) = ops::exec_raw(engine, &args).await else {
            continue;
        };
        let raw = String::from_utf8_lossy(&result.stdout).into_owned();
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
    let mut in_key = false;
    for file in files {
        let header = file[0];
        let paths: Vec<&str> = header["diff --git ".len()..]
            .split(" b/")
            .map(|part| part.trim_start_matches("a/"))
            .collect();
        let binary = file
            .iter()
            .any(|l| l.starts_with("Binary files ") || l.starts_with("GIT binary patch"));
        if binary || paths.iter().any(|p| hidden(repo, p)) {
            continue;
        }
        for (count, line) in file.iter().enumerate() {
            if count >= MAX_FILE_LINES {
                out.push_str(&format!("{MARKER} ({} more lines)\n", file.len() - count));
                break;
            }
            if line.contains("PRIVATE KEY-----") {
                if line.contains("BEGIN") {
                    in_key = true;
                    out.push_str(&format!("+{REDACTED} private key\n"));
                }
                if line.contains("END") {
                    in_key = false;
                }
                continue;
            }
            if in_key {
                continue;
            }
            let structure = ["diff --git ", "+++ ", "--- ", "@@ ", "index "]
                .iter()
                .any(|prefix| line.starts_with(prefix));
            out.push_str(&if structure {
                (*line).to_owned()
            } else {
                redact(line)
            });
            out.push('\n');
        }
    }
    out
}

fn patterns() -> &'static [(Regex, &'static str)] {
    use std::sync::OnceLock;
    static PATTERNS: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        let make = |expr: &str, with: &'static str| (Regex::new(expr).unwrap(), with);
        vec![
            make(r"\bAKIA[0-9A-Z]{16}\b", REDACTED),
            make(r"\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{20,}", REDACTED),
            make(r"\bgithub_pat_[A-Za-z0-9_]{20,}", REDACTED),
            make(r"\bsk-[A-Za-z0-9_-]{16,}", REDACTED),
            make(r"\bxox[abpsr]-[A-Za-z0-9-]{10,}", REDACTED),
            make(
                r#"(?i)((?:secret|token|passw(?:or)?d|passwd|api[_-]?key|auth|credential)[A-Za-z0-9_]*["']?\s*[:=]\s*["']?)[^\s"',;]{8,}"#,
                "${1}[redacted]",
            ),
            make(r"[A-Za-z0-9+/_=-]{32,}", REDACTED),
        ]
    })
}

/// `line` with token-shaped strings replaced.
pub fn redact(line: &str) -> String {
    let mut text = line.to_owned();
    for (pattern, with) in patterns() {
        text = pattern.replace_all(&text, *with).into_owned();
    }
    text
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
            "sk-abcdefghijklmnopqrstuv",
            "xoxb-1234567890-abcdef",
            "aGVsbG8gd29ybGQgdGhpcyBpcyBhIHRva2Vu0123456789",
        ] {
            let out = redact(&format!("+use = \"{secret}\""));
            assert!(!out.contains(secret), "{out}");
        }
        assert!(!redact("+password = hunter2hunter2").contains("hunter2"));
        assert_eq!(redact("+let total = count + 1;"), "+let total = count + 1;");
    }

    const RAW: &str = "diff --git a/src/app.rs b/src/app.rs\n--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1 +1 @@\n-old\n+new\n\
diff --git a/.env b/.env\n+++ b/.env\n+TOKEN=abc\n\
diff --git a/deploy.key b/deploy.key\n+++ b/deploy.key\n+x\n\
diff --git a/logo.png b/logo.png\nBinary files a/logo.png and b/logo.png differ\n\
diff --git a/notes.txt b/notes.txt\n+++ b/notes.txt\n+-----BEGIN RSA PRIVATE KEY-----\n+MIIEabc\n+-----END RSA PRIVATE KEY-----\n+after\n";

    #[test]
    fn drops_credential_files_binaries_and_key_blocks() {
        let out = process("/home/dev/app", RAW);
        assert!(out.contains("+new") && out.contains("+after"));
        for gone in [
            "TOKEN=abc",
            "deploy.key",
            "Binary files",
            "MIIEabc",
            "logo.png",
        ] {
            assert!(!out.contains(gone), "{gone} in {out}");
        }
        assert!(out.contains("private key"));
    }

    #[test]
    fn caps_each_file_and_all_of_it() {
        let mut raw = String::from("diff --git a/big.txt b/big.txt\n+++ b/big.txt\n");
        for i in 0..1000 {
            raw.push_str(&format!("+line {i}\n"));
        }
        let out = process("/r", &raw);
        assert!(out.lines().count() <= MAX_FILE_LINES + 1);
        assert!(out.contains(MARKER));
        let capped = cap_total("x".repeat(MAX_TOTAL_BYTES * 2));
        assert!(capped.len() < MAX_TOTAL_BYTES + 20 && capped.contains(MARKER));
    }

    #[test]
    fn the_script_excludes_credential_patterns_and_reads_both_ranges() {
        let text = script();
        assert!(text.contains(
            "-c core.pager=cat -c diff.external= diff --no-color --no-ext-diff --no-textconv -U3"
        ));
        assert!(text.contains("\"$2..HEAD\"") && text.contains(" HEAD -- ."));
        assert!(text.contains("':(exclude,glob)**/.env'"));
    }
}
