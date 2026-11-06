//! `silo --in-guest`: the client a computer's own agent runs. It reaches the engine through
//! the bridge folder (`/.msb/silo-bridge`) with the token installed at
//! `/etc/silo/guest-client.json`, and can fork its computer, read diffs and ask for review.
//! Keeping, making a fork main, undoing, dropping and secrets are not offered here, and the
//! engine refuses them whatever this client sends (`docs/SiloUI-IN-GUEST-CLIENT.md`).

use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use serde::Deserialize;
use serde_json::{json, Value};
use silo_api::methods::{
    guest_client::{
        GuestRequestFrame, GuestResponseFrame, GUEST_SPOOL_DIR, GUEST_TOKEN_PATH, MAX_FRAME_BYTES,
    },
    review_requests::bucket_counts,
};

use crate::{
    cli::{Command, DiffCommand, FoldCommand, ForkCommand, RequestsAction, RequestsCommand},
    error::CliError,
    summary,
};

const POLL: Duration = Duration::from_millis(100);
const CALL_WAIT: Duration = Duration::from_secs(60);
const JOB_WAIT: Duration = Duration::from_secs(1800);

#[derive(Deserialize)]
struct TokenFile {
    token: String,
    #[serde(rename = "computerId")]
    computer_id: String,
    #[serde(default)]
    spool: Option<String>,
}

/// The token file's path: `SILO_GUEST_CLIENT_FILE`, else the fixed one.
pub fn token_path() -> PathBuf {
    std::env::var_os("SILO_GUEST_CLIENT_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(GUEST_TOKEN_PATH))
}

pub struct Link {
    token: String,
    pub computer_id: String,
    spool: PathBuf,
}

impl Link {
    pub fn load(path: &Path) -> Result<Self, CliError> {
        let text = fs::read_to_string(path).map_err(|error| {
            CliError::new(format!(
                "Cannot read {}: {error}. This computer has no Silo token; ask the person who runs Silo to run `silo guest-client install`.",
                path.display()
            ))
        })?;
        let file: TokenFile = serde_json::from_str(&text)
            .map_err(|error| CliError::new(format!("{} is not valid: {error}", path.display())))?;
        Ok(Self {
            token: file.token,
            computer_id: file.computer_id,
            spool: PathBuf::from(file.spool.unwrap_or_else(|| GUEST_SPOOL_DIR.into())),
        })
    }

    /// One request through the bridge folder, waiting for its answer.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value, CliError> {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let frame = GuestRequestFrame {
            v: 1,
            id: id.clone(),
            token: self.token.clone(),
            method: method.to_owned(),
            params,
        };
        let bytes = serde_json::to_vec(&frame).map_err(|error| CliError::new(error.to_string()))?;
        if bytes.len() > MAX_FRAME_BYTES {
            return Err(CliError::new(
                "The request is larger than the bridge allows.",
            ));
        }
        let io = |error: std::io::Error| {
            CliError::new(format!(
                "Cannot reach the Silo bridge at {}: {error}",
                self.spool.display()
            ))
        };
        let temporary = self.spool.join("req").join(format!("{id}.json.tmp"));
        let target = self.spool.join("req").join(format!("{id}.json"));
        {
            use std::{io::Write, os::unix::fs::OpenOptionsExt};
            // Readable by this user only: the frame carries the token.
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)
                .map_err(io)?;
            file.write_all(&bytes).map_err(io)?;
        }
        fs::rename(&temporary, &target).map_err(io)?;
        let answer = self.spool.join("resp").join(format!("{id}.json"));
        let begun = Instant::now();
        loop {
            match read_answer(&answer) {
                Ok(Some(reply)) => return into_result(reply),
                Ok(None) => {}
                Err(error) => return Err(error),
            }
            if begun.elapsed() > CALL_WAIT {
                return Err(CliError::new(
                    "Silo did not answer in time. Is the computer running with its engine open?",
                ));
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// Starts a job and waits for it, then acknowledges it.
    pub async fn job(&self, method: &str, params: Value, label: &str) -> Result<Value, CliError> {
        let started = self.call(method, params).await?;
        let job = started["jobId"]
            .as_str()
            .ok_or_else(|| CliError::new("The engine did not accept the job."))?
            .to_owned();
        eprintln!("{label}...");
        let begun = Instant::now();
        loop {
            let reply = self
                .call("operations.job.get", json!({ "jobId": job }))
                .await?;
            if reply["state"] == "finished" {
                let outcome = reply.get("outcome").cloned().unwrap_or(Value::Null);
                let _ = self
                    .call("operations.job.acknowledge", json!({ "jobId": job }))
                    .await;
                if let Some(error) = outcome.get("error").filter(|error| !error.is_null()) {
                    return Err(CliError::new(
                        error["message"].as_str().unwrap_or("The operation failed."),
                    ));
                }
                return Ok(outcome.get("result").cloned().unwrap_or(Value::Null));
            }
            if begun.elapsed() > JOB_WAIT {
                return Err(CliError::new(format!("{label} is still running as {job}.")));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
}

fn read_answer(path: &Path) -> Result<Option<GuestResponseFrame>, CliError> {
    let Ok(file) = fs::File::open(path) else {
        return Ok(None);
    };
    let mut bytes = Vec::new();
    file.take(MAX_FRAME_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| CliError::new(error.to_string()))?;
    let _ = fs::remove_file(path);
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| CliError::new(format!("The answer was not readable: {error}")))
}

fn into_result(reply: GuestResponseFrame) -> Result<Value, CliError> {
    if reply.ok {
        return Ok(reply.result.unwrap_or(Value::Null));
    }
    let error = reply.error;
    Err(CliError::new(error.map_or_else(
        || "The request failed.".into(),
        |error| error.message,
    )))
}

fn not_here(what: &str) -> CliError {
    CliError::new(format!(
        "{what} is not available from inside a computer. Fork and ask for review with `silo fold <fork> --request`; the person who runs Silo keeps, promotes or drops."
    ))
}

fn nodes(tree: &Value) -> Vec<Value> {
    tree["nodes"].as_array().cloned().unwrap_or_default()
}

fn find<'a>(nodes: &'a [Value], name: &str) -> Result<&'a Value, CliError> {
    nodes
        .iter()
        .find(|node| node["name"] == name || node["id"] == name)
        .ok_or_else(|| {
            CliError::new(format!(
                "No computer named {name} among this computer's forks."
            ))
        })
}

fn print(json: bool, value: &Value, text: impl FnOnce() -> String) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(value).unwrap_or_default()
        );
    } else {
        println!("{}", text());
    }
}

/// Runs one command through the bridge.
pub async fn run(command: Command, json: bool, link: &Link) -> Result<i32, CliError> {
    match command {
        Command::List { .. } | Command::Tree => {
            let tree = link.call("computers.tree", json!({})).await?;
            print(json, &tree, || {
                nodes(&tree)
                    .iter()
                    .map(|node| {
                        let parent = node["parent"].as_str().map_or(String::new(), |parent| {
                            let name = nodes(&tree)
                                .iter()
                                .find(|other| other["id"] == parent)
                                .and_then(|other| other["name"].as_str().map(str::to_owned))
                                .unwrap_or_else(|| parent.to_owned());
                            format!("  (fork of {name})")
                        });
                        format!(
                            "{}  {}{}",
                            node["name"].as_str().unwrap_or_default(),
                            node["state"].as_str().unwrap_or_default(),
                            parent
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            });
            Ok(0)
        }
        Command::Status { computer } => {
            let tree = link.call("computers.tree", json!({})).await?;
            let all = nodes(&tree);
            let node = find(&all, &computer)?;
            print(json, node, || {
                format!(
                    "{}: {}",
                    node["name"].as_str().unwrap_or_default(),
                    node["state"].as_str().unwrap_or_default()
                )
            });
            Ok(0)
        }
        Command::Fork(fork) => fork_self(fork, json, link).await,
        Command::Fold(fold) => request_review(fold, json, link).await,
        Command::Requests(requests) => requests_command(requests, json, link).await,
        Command::Diff(diff) => diff_fork(diff, json, link).await,
        Command::Steps { computer } => {
            let id = resolved_id(link, &computer).await?;
            let steps = link.call("steps.list", json!({ "computerId": id })).await?;
            print(json, &steps, || {
                serde_json::to_string_pretty(&steps).unwrap_or_default()
            });
            Ok(0)
        }
        Command::Undo { .. } | Command::Drop(_) | Command::Secrets(_) => Err(not_here("That")),
        _ => Err(not_here("That command")),
    }
}

async fn resolved_id(link: &Link, name: &str) -> Result<String, CliError> {
    let tree = link.call("computers.tree", json!({})).await?;
    let all = nodes(&tree);
    Ok(find(&all, name)?["id"]
        .as_str()
        .unwrap_or_default()
        .to_owned())
}

async fn fork_self(fork: ForkCommand, json: bool, link: &Link) -> Result<i32, CliError> {
    if fork.count.is_some()
        || !fork.command.is_empty()
        || fork.recipe.is_some()
        || fork.checkpoint.is_some()
        || fork.from_step.is_some()
        || fork.recipe_file.is_some()
    {
        return Err(not_here(
            "Forking with --recipe, --checkpoint, --from-step, -n or a command",
        ));
    }
    let tree = link.call("computers.tree", json!({})).await?;
    let all = nodes(&tree);
    let me = all
        .iter()
        .find(|node| node["id"] == link.computer_id.as_str())
        .ok_or_else(|| CliError::new("This computer is not in the engine's list."))?;
    let my_name = me["name"].as_str().unwrap_or_default().to_owned();
    if fork.source != "." && fork.source != my_name && fork.source != link.computer_id {
        return Err(CliError::new(format!(
            "A computer's own agent can fork only that computer ({my_name}), not {}.",
            fork.source
        )));
    }
    let name = fork.name.unwrap_or_else(|| {
        format!(
            "{my_name}-{}",
            &uuid::Uuid::new_v4().simple().to_string()[..6]
        )
    });
    link.job(
        "checkpoints.fork",
        json!({ "computerId": link.computer_id, "newName": name, "secrets": fork.secrets }),
        &format!("Forking {my_name} into {name}"),
    )
    .await?;
    let id = resolved_id(link, &name).await?;
    link.job(
        "computers.start",
        json!({ "computerId": id }),
        &format!("Starting {name}"),
    )
    .await?;
    let result = json!({ "fork": { "id": id, "name": name }, "source": my_name });
    print(json, &result, || {
        format!("Forked {my_name} into {name}. Review with: silo diff {name}")
    });
    Ok(0)
}

async fn diff_fork(diff: DiffCommand, json: bool, link: &Link) -> Result<i32, CliError> {
    let id = resolved_id(link, &diff.computer).await?;
    let mut params = json!({ "computerId": id });
    if let Some(lines) = diff.max_lines {
        params["maxLines"] = json!(lines);
    }
    if diff.no_baseline {
        params["baseline"] = json!(false);
    }
    if diff.baseline {
        return Err(not_here("Measuring a baseline"));
    }
    let seen = link
        .job(
            "changes.diff",
            params,
            &format!("Comparing {}", diff.computer),
        )
        .await?;
    print(json, &seen, || {
        seen["text"].as_str().unwrap_or("No changes.").to_owned()
    });
    Ok(0)
}

async fn request_review(fold: FoldCommand, json: bool, link: &Link) -> Result<i32, CliError> {
    if !fold.request
        || fold.into.is_some()
        || fold.dry_run
        || fold.promote
        || fold.data_ok
        || fold.reviewed.is_some()
        || fold.recipe_file.is_some()
        || fold.pr
    {
        return Err(not_here("Keeping, promoting or opening a pull request"));
    }
    let id = resolved_id(link, &fold.fork).await?;
    let seen = link
        .job(
            "changes.diff",
            json!({ "computerId": id }),
            &format!("Comparing {}", fold.fork),
        )
        .await?;
    let digest = seen["reviewDigest"].as_str().unwrap_or_default().to_owned();
    let mut params = json!({
        "forkId": id,
        "summary": summary::review(&seen),
        "counts": bucket_counts(&seen),
    });
    if !digest.is_empty() {
        params["reviewDigest"] = json!(digest);
    }
    let mut value = link.call("changes.request", params).await?;
    value["fork"] = json!(fold.fork);
    value["status"] = json!("requested");
    print(json, &value, || {
        format!(
            "Asked for a review of {}: {}",
            fold.fork,
            value["request"]["id"].as_str().unwrap_or_default()
        )
    });
    Ok(0)
}

async fn requests_command(
    requests: RequestsCommand,
    json: bool,
    link: &Link,
) -> Result<i32, CliError> {
    match requests.action {
        None => {
            let status = requests.status.unwrap_or_else(|| "open".into());
            let listed = link
                .call("changes.requests.list", json!({ "status": status }))
                .await?;
            print(json, &listed, || {
                listed["requests"]
                    .as_array()
                    .map(|entries| {
                        entries
                            .iter()
                            .map(|entry| {
                                format!(
                                    "{}  {}  {}",
                                    entry["request"]["id"].as_str().unwrap_or_default(),
                                    entry["forkName"].as_str().unwrap_or_default(),
                                    entry["request"]["status"].as_str().unwrap_or_default()
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default()
            });
            Ok(0)
        }
        Some(RequestsAction::Show { review: true, .. }) => {
            Err(not_here("Sending a diff to the hosted reviewer"))
        }
        Some(RequestsAction::Show { id, .. }) => {
            let shown = link
                .job(
                    "changes.requests.show",
                    json!({ "requestId": id }),
                    &format!("Reading {id}"),
                )
                .await?;
            print(json, &shown, || {
                serde_json::to_string_pretty(&shown).unwrap_or_default()
            });
            Ok(0)
        }
        Some(_) => Err(not_here("Keeping, throwing away or opening a request")),
    }
}

#[cfg(test)]
mod tests;
