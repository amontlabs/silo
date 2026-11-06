//! `silo fold <fork> --pr`: the fork's code on a new branch, pushed from this device, and a
//! pull request that carries the review summary.
//!
//! The push and the API call run on the host with credentials the host already has: the
//! `GITHUB_TOKEN` of the environment (a CI runner sets it) or the token `gh` holds, and the
//! host's own git configuration. Nothing here reads a fork's credentials, and a fork's GitHub
//! access stays read-only.

use serde_json::{json, Value};

use crate::{
    engine::{Engine, Progress},
    error::CliError,
    folder::{self, Git, PrSource},
    ops::{self, DiffArgs},
    summary,
};

const DEFAULT_API: &str = "https://api.github.com";

/// What the command line adds to `--pr`.
#[derive(Clone, Debug, Default)]
pub struct PrArgs {
    /// `OWNER/NAME` of the repository to open the pull request in.
    pub repo: Option<String>,
    /// The branch to merge into.
    pub base: Option<String>,
}

/// What the process around us provides.
#[derive(Clone, Debug)]
pub struct PrEnv {
    pub token: Option<String>,
    pub api_url: String,
    /// `GITHUB_REPOSITORY` (set on a CI runner).
    pub repository: Option<String>,
}

impl PrEnv {
    pub fn from_process() -> Self {
        let set = |name: &str| {
            std::env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
        };
        Self {
            token: set("GITHUB_TOKEN").or_else(gh_token),
            api_url: set("SILO_GITHUB_API_URL").unwrap_or_else(|| DEFAULT_API.to_owned()),
            repository: set("GITHUB_REPOSITORY"),
        }
    }
}

/// The token `gh auth token` prints, when `gh` is installed and signed in.
fn gh_token() -> Option<String> {
    let output = std::process::Command::new("gh")
        .args(["auth", "token"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let token = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (output.status.success() && !token.is_empty()).then_some(token)
}

/// `owner/name` from a remote URL (`https://host/owner/name.git`, `git@host:owner/name.git`,
/// `ssh://git@host/owner/name`); a path on this device has none.
pub fn repository_of(remote: &str) -> Option<String> {
    let path = if let Some((_, rest)) = remote.split_once("://") {
        rest.split_once('/')?.1
    } else if let Some((user_host, path)) = remote.split_once(':') {
        if !user_host.contains('@') {
            return None;
        }
        path
    } else {
        return None;
    };
    let path = path.trim_matches('/').trim_end_matches(".git");
    let mut parts = path.rsplit('/');
    let (name, owner) = (parts.next()?, parts.next()?);
    (!name.is_empty() && !owner.is_empty()).then(|| format!("{owner}/{name}"))
}

/// The URL without a user name or password, for messages.
fn redact(url: &str) -> String {
    match url.split_once("://") {
        Some((scheme, rest)) => {
            let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
            let host = authority.rsplit('@').next().unwrap_or(authority);
            format!("{scheme}://{host}/{path}")
        }
        None => url.to_owned(),
    }
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Environment for a push to an `https` remote: the token goes in an HTTP header scoped to
/// that remote's origin, never in an argument or the URL.
fn auth_env(remote: &str, token: &str) -> Vec<(String, String)> {
    let Some(rest) = remote.strip_prefix("https://") else {
        return Vec::new();
    };
    let host = rest.split('/').next().unwrap_or("");
    let host = host.rsplit('@').next().unwrap_or(host);
    vec![
        ("GIT_CONFIG_COUNT".into(), "1".into()),
        (
            "GIT_CONFIG_KEY_0".into(),
            format!("http.https://{host}/.extraheader"),
        ),
        (
            "GIT_CONFIG_VALUE_0".into(),
            format!(
                "Authorization: Basic {}",
                base64(format!("x-access-token:{token}").as_bytes())
            ),
        ),
    ]
}

async fn push(source: &PrSource, token: &str) -> Result<(), CliError> {
    let git = Git {
        dir: source.directory.clone(),
    };
    let refspec = format!("refs/heads/{0}:refs/heads/{0}", source.branch);
    let env = auth_env(&source.remote, token);
    let output = git
        .run_env(&["push", "--quiet", &source.remote, &refspec], &env)
        .await?;
    if output.code != 0 {
        return Err(CliError::new(format!(
            "Pushing {} to {} failed: {}",
            source.branch,
            redact(&source.remote),
            output.err.replace(token, "***").trim()
        )));
    }
    Ok(())
}

struct Opened {
    url: String,
    number: u64,
}

/// `POST /repos/{repo}/pulls`.
fn create_pull_request(
    env: &PrEnv,
    token: &str,
    repo: &str,
    title: &str,
    head: &str,
    base: &str,
    body: &str,
) -> Result<Opened, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| error.to_string())?;
    let url = format!("{}/repos/{repo}/pulls", env.api_url.trim_end_matches('/'));
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "silo-cli")
        .json(&json!({"title": title, "head": head, "base": base, "body": body}))
        .send()
        .map_err(|error| format!("Could not reach {}: {}", env.api_url, error.without_url()))?;
    let status = response.status();
    let reply: Value = response.json().unwrap_or(Value::Null);
    if status.as_u16() != 201 {
        let message = reply["message"].as_str().unwrap_or("no message");
        let details: Vec<&str> = reply["errors"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|error| error["message"].as_str())
            .collect();
        return Err(format!(
            "GitHub answered {status} to the pull request: {message}{}",
            if details.is_empty() {
                String::new()
            } else {
                format!(" ({})", details.join("; "))
            }
        ));
    }
    Ok(Opened {
        url: reply["html_url"].as_str().unwrap_or_default().to_owned(),
        number: reply["number"].as_u64().unwrap_or(0),
    })
}

/// A fork of a computer: its repository's work bundled in the fork, fetched into a temporary
/// repository on this device, ready to push.
async fn computer_source(
    engine: &Engine,
    fork: &str,
    diff: &Value,
    branch: &str,
    progress: Progress<'_>,
) -> Result<(PrSource, Vec<String>), CliError> {
    let repos: Vec<&str> = diff["code"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| entry["removed"] != true)
        .filter_map(|entry| entry["repo"].as_str())
        .collect();
    let Some(repo) = repos.first().copied() else {
        return Err(CliError::new(format!(
            "{fork} has no code changes to open a pull request for."
        )));
    };
    let notes: Vec<String> = repos
        .iter()
        .skip(1)
        .map(|other| format!("{other} also changed; the pull request holds {repo} only."))
        .collect();
    let computer = engine.computer(fork).await?;
    let started_here = match computer.state.as_str() {
        "running" => false,
        "stopped" => {
            ops::start(engine, fork, progress).await?;
            true
        }
        other => {
            return Err(CliError::new(format!(
                "{fork} is {other}; wait until it is running or stopped, then try again."
            )))
        }
    };
    let made = computer_bundle(engine, fork, repo, branch, progress).await;
    if started_here {
        let stopped = ops::stop(engine, fork, progress).await;
        let source = made?;
        stopped?;
        return Ok((source, notes));
    }
    Ok((made?, notes))
}

async fn computer_bundle(
    engine: &Engine,
    fork: &str,
    repo: &str,
    branch: &str,
    progress: Progress<'_>,
) -> Result<PrSource, CliError> {
    let computer = engine.computer(fork).await?;
    let outbox = format!(
        "{}/silo-out-{}",
        folder::TRANSFER_ROOT,
        uuid::Uuid::new_v4().simple()
    );
    let gather = "set -e; cd \"$1\"; git add -A; \
                  git diff --cached --quiet || git \
                  -c user.name=\"$(git config user.name || echo Silo)\" \
                  -c user.email=\"$(git config user.email || echo silo@localhost)\" \
                  commit -q -m \"silo: uncommitted changes in $2\"; \
                  mkdir -p \"$3\"; git bundle create --quiet \"$3/out.bundle\" HEAD; \
                  git remote get-url origin 2>/dev/null || echo; \
                  git symbolic-ref --short -q refs/remotes/origin/HEAD 2>/dev/null || echo";
    let found = folder::guest(
        engine,
        fork,
        gather,
        &[repo, fork, &outbox],
        "Collecting the commits",
    )
    .await?;
    let mut lines = found.lines();
    let remote = lines.next().unwrap_or("").trim().to_owned();
    let base = lines
        .next()
        .map(|line| line.trim().trim_start_matches("origin/").to_owned())
        .filter(|line| !line.is_empty());
    let cleanup = |outbox: String| async move {
        let _ = folder::guest(engine, fork, "rm -rf \"$1\"", &[&outbox], "Cleaning up").await;
    };
    if remote.is_empty() {
        cleanup(outbox).await;
        return Err(CliError::new(format!(
            "The repository {repo} in {fork} has no origin remote to push the branch to."
        )));
    }
    let directory = std::env::temp_dir().join(format!("silo-pr-{}", uuid::Uuid::new_v4().simple()));
    let result = async {
        std::fs::create_dir_all(&directory).map_err(|error| {
            CliError::new(format!("Cannot create {}: {error}", directory.display()))
        })?;
        let git = Git {
            dir: directory.clone(),
        };
        git.ok(&["init", "--quiet", "--bare"]).await?;
        let bundle = directory.join("out.bundle");
        folder::download(
            engine,
            &computer,
            &format!("{outbox}/out.bundle"),
            &bundle,
            progress,
        )
        .await?;
        let bundle_arg = bundle.to_string_lossy().into_owned();
        git.ok(&[
            "fetch",
            "--quiet",
            &bundle_arg,
            &format!("+HEAD:refs/heads/{branch}"),
        ])
        .await?;
        Ok::<_, CliError>(())
    }
    .await;
    cleanup(outbox).await;
    if let Err(error) = result {
        let _ = std::fs::remove_dir_all(&directory);
        return Err(error);
    }
    Ok(PrSource {
        directory,
        branch: branch.to_owned(),
        base,
        remote,
    })
}

/// Opens a pull request for a fork's code changes. The result carries `branch`, `base`,
/// `repository` and `pullRequest` (`url`, `number`). Without a token nothing is pushed.
pub async fn open(
    engine: &Engine,
    fork: &str,
    args: &PrArgs,
    env: &PrEnv,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let token = env.token.clone().ok_or_else(|| {
        CliError::new(
            "A pull request needs a GitHub token on this device: set GITHUB_TOKEN, or sign in with `gh auth login`. Nothing was pushed.",
        )
    })?;
    let computer = engine.computer(fork).await?;
    let branch = format!(
        "silo/{}-{}",
        folder::ref_label(&computer.name),
        ops::short_id()
    );
    let record = folder::record_for(engine, &computer.id);
    let (source, text, notes) = match &record {
        Some(record) => {
            let text = folder::review_summary(engine, &computer, record).await?;
            let source =
                folder::branch_for_pr(engine, &computer, record, &branch, progress).await?;
            (source, format!("Code\n{text}"), Vec::new())
        }
        None => {
            let seen = ops::diff(
                engine,
                &DiffArgs {
                    computer: computer.name.clone(),
                    max_lines: None,
                    no_baseline: false,
                    compute_baseline: false,
                },
                progress,
            )
            .await?;
            let (source, notes) =
                computer_source(engine, &computer.name, &seen, &branch, progress).await?;
            (source, summary::review(&seen), notes)
        }
    };
    let outcome = publish(&source, args, env, &token, &computer.name, &text).await;
    if record.is_none() {
        let _ = std::fs::remove_dir_all(&source.directory);
    }
    let mut value = outcome?;
    value["fork"] = json!(computer.name);
    value["notes"] = json!(notes);
    Ok(value)
}

async fn publish(
    source: &PrSource,
    args: &PrArgs,
    env: &PrEnv,
    token: &str,
    fork: &str,
    text: &str,
) -> Result<Value, CliError> {
    let repository = args
        .repo
        .clone()
        .or_else(|| env.repository.clone())
        .or_else(|| repository_of(&source.remote))
        .ok_or_else(|| {
            CliError::new(format!(
                "Cannot tell which GitHub repository {} is; pass --repo OWNER/NAME.",
                redact(&source.remote)
            ))
        })?;
    let base = args
        .base
        .clone()
        .or_else(|| source.base.clone())
        .unwrap_or_else(|| "main".to_owned());
    push(source, token).await?;
    let title = format!("Changes from {fork}");
    let body = format!("{text}\n\n---\nOpened by `silo fold --pr` from the fork {fork}.");
    let env = env.clone();
    let (token, head) = (token.to_owned(), source.branch.clone());
    let (repo, base_branch) = (repository.clone(), base.clone());
    let opened = tokio::task::spawn_blocking(move || {
        create_pull_request(&env, &token, &repo, &title, &head, &base_branch, &body)
    })
    .await
    .map_err(|_| CliError::new("The pull request call stopped unexpectedly."))?
    .map_err(|message| {
        CliError::new(format!(
            "{message} The branch {} was pushed; open the pull request by hand.",
            source.branch
        ))
    })?;
    Ok(json!({
        "status": "pull-request",
        "branch": source.branch,
        "base": base,
        "repository": repository,
        "remote": redact(&source.remote),
        "prUrl": opened.url.clone(),
        "pullRequest": {"url": opened.url, "number": opened.number},
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_repository_comes_from_remote_urls_and_not_from_paths() {
        for url in [
            "https://github.com/acme/app.git",
            "https://github.com/acme/app",
            "git@github.com:acme/app.git",
            "ssh://git@github.com/acme/app.git",
            "https://x-access-token:abc@github.com/acme/app.git",
        ] {
            assert_eq!(repository_of(url).as_deref(), Some("acme/app"), "{url}");
        }
        assert_eq!(repository_of("/tmp/remote.git"), None);
        assert_eq!(repository_of("../remote.git"), None);
        assert_eq!(repository_of("https://github.com/app"), None);
    }

    #[test]
    fn base64_matches_the_standard_alphabet() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"x-access-token:t"), "eC1hY2Nlc3MtdG9rZW46dA==");
    }

    #[test]
    fn the_token_travels_in_a_header_for_https_remotes_only() {
        let env = auth_env("https://github.com/acme/app.git", "tok");
        assert_eq!(env[1].1, "http.https://github.com/.extraheader");
        assert!(env[2].1.starts_with("Authorization: Basic "));
        assert!(!env
            .iter()
            .any(|(_, value)| value.contains("tok") && !value.starts_with("Authorization")));
        assert!(auth_env("git@github.com:acme/app.git", "tok").is_empty());
        assert!(auth_env("/tmp/remote.git", "tok").is_empty());
        assert_eq!(
            redact("https://x-access-token:abc@github.com/acme/app.git"),
            "https://github.com/acme/app.git"
        );
        assert_eq!(redact("/tmp/remote.git"), "/tmp/remote.git");
    }
}
