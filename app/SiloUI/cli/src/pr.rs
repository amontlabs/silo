//! `silo fold <fork> --pr`: the fork's code on a new branch, pushed from this device, and a
//! pull request that carries the review summary.
//!
//! The push and the API call run on the host with credentials the host already has: the
//! `GITHUB_TOKEN` of the environment (a CI runner sets it) or the token `gh` holds, and the
//! host's own git configuration. Nothing here reads a fork's credentials, and a fork's GitHub
//! access stays read-only.
//!
//! The place a branch is pushed to never comes from a fork: a fork controls its own `origin`,
//! and the host token would go wherever that points. For a folder fork it is the host
//! folder's `origin`; for any other fork it is `https://<git host>/<repo>.git`, built on the
//! host from `--repo`, else `GITHUB_REPOSITORY`, else the host project's `origin`.

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
    /// The host project's top folder (the one holding `.silo/`), whose `origin` names the
    /// repository when nothing else does.
    pub host_root: Option<std::path::PathBuf>,
}

/// What the process around us provides.
#[derive(Clone, Debug)]
pub struct PrEnv {
    pub token: Option<String>,
    pub api_url: String,
    /// `GITHUB_REPOSITORY` (set on a CI runner).
    pub repository: Option<String>,
    /// Where repositories are pushed to, like `https://github.com`; follows `api_url`.
    pub git_base: String,
}

/// The git host that goes with a REST base: `https://github.com` for `https://api.github.com`,
/// the host of `https://ghe.example/api/v3`, else the base itself.
pub fn git_base_for(api_url: &str) -> String {
    let api = api_url.trim_end_matches('/');
    if api == DEFAULT_API {
        return "https://github.com".to_owned();
    }
    api.strip_suffix("/api/v3").unwrap_or(api).to_owned()
}

impl PrEnv {
    pub fn from_process() -> Self {
        let set = |name: &str| {
            std::env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
        };
        let api_url = set("SILO_GITHUB_API_URL").unwrap_or_else(|| DEFAULT_API.to_owned());
        Self {
            token: set("GITHUB_TOKEN").or_else(gh_token),
            git_base: git_base_for(&api_url),
            api_url,
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
    if source.remote.starts_with('-') {
        return Err(CliError::new("The push destination is not a repository URL."));
    }
    let output = git
        .run_env(&["push", "--quiet", "--", &source.remote, &refspec], &env)
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

/// Whether `path` is `repo` itself, under it, or a folder that holds it.
fn within(path: &str, repo: &str) -> bool {
    let (path, repo) = (path.trim_end_matches('/'), repo.trim_end_matches('/'));
    path == repo || path.starts_with(&format!("{repo}/")) || repo.starts_with(&format!("{path}/"))
}

/// The credential paths the diff names that lie in `repo`.
fn credentials_in(diff: &Value, repo: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (group, paths) in diff["not_kept"]["credentials"]
        .as_object()
        .into_iter()
        .flatten()
    {
        let listed = paths.as_array().into_iter().flatten().filter_map(Value::as_str);
        for path in std::iter::once(group.as_str()).chain(listed) {
            if within(path, repo) && !found.iter().any(|f| f == path) {
                found.push(path.to_owned());
            }
        }
    }
    found
}

/// `owner/name` with the characters of GitHub names only, so it is a path and nothing else.
fn valid_repository(repository: &str) -> bool {
    let part = |text: &str| {
        !text.is_empty()
            && text != "."
            && text != ".."
            && !text.starts_with('-')
            && text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    };
    repository.split_once('/').is_some_and(|(owner, name)| part(owner) && part(name))
}

/// The repository and push URL for a fork of a computer, decided on this device: `--repo`,
/// else `GITHUB_REPOSITORY`, else the `origin` of the host project; never what the fork says.
async fn computer_destination(
    args: &PrArgs,
    env: &PrEnv,
    host_root: Option<&std::path::Path>,
) -> Result<(String, String), CliError> {
    let mut repository = args.repo.clone().or_else(|| env.repository.clone());
    if repository.is_none() {
        if let Some(root) = host_root {
            let origin = Git {
                dir: root.to_path_buf(),
            }
            .run(&["remote", "get-url", "origin"])
            .await?;
            if origin.code == 0 {
                repository = repository_of(&origin.out);
            }
        }
    }
    let repository = repository.ok_or_else(|| {
        CliError::new(
            "Cannot tell which GitHub repository to push to: pass --repo OWNER/NAME, or set GITHUB_REPOSITORY. A fork's own remote is never used.",
        )
    })?;
    if !valid_repository(&repository) {
        return Err(CliError::new(format!(
            "`{repository}` is not a repository name like OWNER/NAME."
        )));
    }
    let url = format!("{}/{repository}.git", env.git_base.trim_end_matches('/'));
    if !url.starts_with("https://") {
        return Err(CliError::new(format!(
            "Refusing to push to {}: only https destinations are used for a fork of a computer.",
            redact(&url)
        )));
    }
    Ok((repository, url))
}

/// A fork of a computer: its repository's work bundled in the fork, fetched into a temporary
/// repository on this device, ready to push to `url`. Returns the untracked files that were
/// committed with it.
async fn computer_source(
    engine: &Engine,
    fork: &str,
    diff: &Value,
    branch: &str,
    url: &str,
    progress: Progress<'_>,
) -> Result<(PrSource, Vec<String>, Vec<String>), CliError> {
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
    let credentials = credentials_in(diff, repo);
    if !credentials.is_empty() {
        return Err(CliError::new(format!(
            "{repo} in {fork} holds credential paths, which a pull request would publish: {}. Remove them from the fork, then try again.",
            credentials.join(", ")
        )));
    }
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
    let made = computer_bundle(engine, fork, repo, branch, url, progress).await;
    if started_here {
        let stopped = ops::stop(engine, fork, progress).await;
        let (source, untracked) = made?;
        stopped?;
        return Ok((source, untracked, notes));
    }
    let (source, untracked) = made?;
    Ok((source, untracked, notes))
}

async fn computer_bundle(
    engine: &Engine,
    fork: &str,
    repo: &str,
    branch: &str,
    url: &str,
    progress: Progress<'_>,
) -> Result<(PrSource, Vec<String>), CliError> {
    let computer = engine.computer(fork).await?;
    let outbox = format!(
        "{}/silo-out-{}",
        folder::TRANSFER_ROOT,
        uuid::Uuid::new_v4().simple()
    );
    // Prints how many untracked files there are and their names, then commits everything.
    let gather = "set -e; cd \"$1\"; git ls-files -o --exclude-standard | wc -l | tr -d ' '; \
                  git ls-files -o --exclude-standard; git add -A; \
                  git diff --cached --quiet || git \
                  -c user.name=\"$(git config user.name || echo Silo)\" \
                  -c user.email=\"$(git config user.email || echo silo@localhost)\" \
                  commit -q -m \"silo: uncommitted changes in $2\"; \
                  mkdir -p \"$3\"; git bundle create --quiet \"$3/out.bundle\" HEAD";
    let found = folder::guest(
        engine,
        fork,
        gather,
        &[repo, fork, &outbox],
        "Collecting the commits",
    )
    .await?;
    let mut lines = found.lines();
    let count: usize = lines
        .next()
        .and_then(|line| line.trim().parse().ok())
        .unwrap_or(0);
    let untracked: Vec<String> = lines.take(count).map(str::to_owned).collect();
    let cleanup = |outbox: String| async move {
        let _ = folder::guest(engine, fork, "rm -rf \"$1\"", &[&outbox], "Cleaning up").await;
    };
    let directory =
        std::env::temp_dir().join(format!("silo-pr-{}", uuid::Uuid::new_v4().simple()));
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
    let source = PrSource {
        directory,
        branch: branch.to_owned(),
        base: None,
        remote: url.to_owned(),
    };
    Ok((source, untracked))
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
            let (repository, url) =
                computer_destination(args, env, args.host_root.as_deref()).await?;
            let _ = repository;
            let (source, untracked, mut notes) =
                computer_source(engine, &computer.name, &seen, &branch, &url, progress).await?;
            let mut text = summary::review(&seen);
            if !untracked.is_empty() {
                let list = untracked.join(", ");
                text.push_str(&format!("\n\nUntracked files committed: {list}"));
                notes.push(format!("Untracked files were committed: {list}"));
            }
            (source, text, notes)
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
