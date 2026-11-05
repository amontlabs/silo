//! `silo fork <source> -n N` and `silo fork <source> -- <cmd>`: several forks of one
//! checkpoint made at once, each started, optionally running a command and left for review.

use std::time::Duration;

use serde_json::{json, Value};
use silo_api::methods::{
    checkpoints::{CheckpointForkParams, CheckpointKind},
    guest::{GuestExecGetParams, GuestExecStartParams},
    network::NetworkTargetParams,
    requests,
};

use crate::{
    config::Settings,
    engine::{find, Computer, Engine, Progress},
    error::CliError,
    ops::{self, ForkArgs, MAIN},
};

/// Most forks one command makes.
pub const MAX_FORKS: u32 = 32;
/// Apple's limit on macOS virtual machines running at once.
pub const MACOS_VM_LIMIT: usize = 2;
const EXEC_POLL: Duration = Duration::from_millis(250);
const RETAIN_OUTPUT_BYTES: u64 = 8 << 20;
/// How much of a command's output the result keeps for each stream.
const KEEP_OUTPUT_BYTES: usize = 256 * 1024;

/// Where a command's output goes as it arrives: fork name, whether it is stderr, the bytes.
pub type Output<'a> = &'a (dyn Fn(&str, bool, &[u8]) + Sync);

#[derive(Clone, Debug, Default)]
pub struct ManyArgs {
    /// The source, the name (a prefix when several), checkpoint, secrets and recipe.
    pub fork: ForkArgs,
    pub count: u32,
    /// Run in each fork once it is started; empty means none.
    pub command: Vec<String>,
    pub cwd: Option<String>,
}

fn gib(bytes: u64) -> String {
    let tenths = (bytes as f64 / (1u64 << 30) as f64 * 10.0).round() as u64;
    if tenths % 10 == 0 {
        format!("{} GiB", tenths / 10)
    } else {
        format!("{}.{} GiB", tenths / 10, tenths % 10)
    }
}

/// The names for `count` forks: the source name plus a short id each, the given name alone
/// for one, or the given prefix with `-1`, `-2` and so on. None may be taken.
pub fn fork_names(
    source: &str,
    name: Option<&str>,
    count: u32,
    existing: &[Computer],
    mut short_id: impl FnMut() -> String,
) -> Result<Vec<String>, CliError> {
    let names: Vec<String> = match (name, count) {
        (Some(name), 1) => vec![name.to_owned()],
        (Some(prefix), count) => (1..=count).map(|n| format!("{prefix}-{n}")).collect(),
        (None, count) => (0..count)
            .map(|_| ops::default_fork_name(source, &short_id()))
            .collect(),
    };
    for name in &names {
        if existing.iter().any(|computer| &computer.name == name) {
            return Err(CliError::new(format!(
                "A computer called {name} already exists; pick another --name."
            )));
        }
    }
    Ok(names)
}

/// Refuses forks that do not fit: Apple's two macOS virtual machines, then `fork_ram_budget`.
///
/// The engine does not report how much memory a running computer uses, so a running fork
/// counts as the memory it was created with, and so does each new one. Running forks are
/// every running computer except `main`.
pub fn check_capacity(
    source: &Computer,
    all: &[Computer],
    count: u32,
    budget: Option<u64>,
) -> Result<(), CliError> {
    if source.kind == "macos" {
        let running = all
            .iter()
            .filter(|computer| computer.kind == "macos" && computer.state == "running")
            .count();
        let free = MACOS_VM_LIMIT.saturating_sub(running);
        if count as usize > free {
            return Err(CliError::new(format!(
                "Apple allows {MACOS_VM_LIMIT} macOS virtual machines at once and {running} already run, \
                 so {free} more can start; {count} forks need {count}. Fork a Linux computer for more, \
                 or stop a macOS computer first."
            )));
        }
    }
    let Some(budget) = budget else {
        return Ok(());
    };
    let each = source.memory_gib << 30;
    let running: Vec<&Computer> = all
        .iter()
        .filter(|computer| computer.state == "running" && computer.name != MAIN)
        .collect();
    let used: u64 = running
        .iter()
        .map(|computer| computer.memory_gib << 30)
        .sum();
    let needed = used + each * u64::from(count);
    if needed <= budget {
        return Ok(());
    }
    let fits = if each == 0 {
        0
    } else {
        budget.saturating_sub(used) / each
    };
    let fits = match fits {
        0 => "none fits".to_owned(),
        1 => "1 fits".to_owned(),
        n => format!("{n} fit"),
    };
    Err(CliError::new(format!(
        "fork_ram_budget is {}: {} running fork{} use {} and {count} more at {} each need {}, so {fits}. \
         Lower -n, stop forks you no longer need, or raise fork_ram_budget in .silo/silo.yaml. \
         The sizes are the memory the computers were created with.",
        gib(budget),
        running.len(),
        if running.len() == 1 { "" } else { "s" },
        gib(used),
        gib(each),
        gib(each * u64::from(count)),
    )))
}

/// The URLs of a computer's published websites.
pub async fn urls(engine: &Engine, computer: &Computer) -> Vec<String> {
    engine
        .call(requests::network_urls(NetworkTargetParams {
            computer_id: computer.id.clone(),
            device_id: None,
        }))
        .await
        .ok()
        .and_then(|reply| reply.get("urls").and_then(Value::as_array).cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|entry| entry["url"].as_str().map(str::to_owned))
        .collect()
}

/// What a streamed command did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ran {
    pub exit_code: i32,
    pub signal: Option<String>,
}

/// Runs a command with no time limit, passing its output on as it arrives.
pub async fn exec_streaming(
    engine: &Engine,
    computer: &Computer,
    argv: &[String],
    cwd: Option<&str>,
    progress: Progress<'_>,
    mut sink: impl FnMut(bool, &[u8]),
) -> Result<Ran, CliError> {
    let started = engine
        .call(requests::guest_exec_start(GuestExecStartParams {
            computer_id: computer.id.clone(),
            device_id: None,
            argv: argv.to_vec(),
            cwd: cwd.map(str::to_owned),
            env: None,
            as_user: None,
            stdin: None,
            stdin_open: false,
            retain_output_bytes: RETAIN_OUTPUT_BYTES,
        }))
        .await?;
    let job = started
        .get("jobId")
        .and_then(Value::as_str)
        .ok_or_else(|| CliError::new("The engine did not start the command."))?
        .to_owned();
    let job_id = silo_api::JobId::new(&job)
        .map_err(|_| CliError::new("The engine sent an invalid job id."))?;
    let mut seen = 0u64;
    loop {
        let reply = engine
            .call(requests::guest_exec_get(GuestExecGetParams {
                job_id: job_id.clone(),
                device_id: None,
                since_seq: Some(seen),
            }))
            .await?;
        for chunk in reply["output"].as_array().into_iter().flatten() {
            let bytes: Vec<u8> = chunk["data"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|byte| byte.as_u64().map(|byte| byte as u8))
                .collect();
            seen = seen.max(chunk["seq"].as_u64().unwrap_or(0));
            sink(chunk["stream"] == "stderr", &bytes);
        }
        match reply["state"].as_str() {
            Some("running") => tokio::time::sleep(EXEC_POLL).await,
            Some("exited") => break,
            Some(other) => {
                return Err(CliError::new(format!(
                    "The command on {} did not finish ({other}).",
                    computer.name
                )))
            }
            None => return Err(CliError::new("The engine's reply had no state.")),
        }
    }
    let finished = engine
        .wait_job(
            &job,
            &format!("Finishing the command on {}", computer.name),
            progress,
        )
        .await?;
    let code = finished
        .get("exitCode")
        .and_then(Value::as_i64)
        .map(|c| c as i32);
    let signal = finished
        .get("signal")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Ok(Ran {
        exit_code: ops::exit_code_for(code, signal.as_deref()),
        signal,
    })
}

/// Keeps the first `KEEP_OUTPUT_BYTES` of a stream.
#[derive(Default)]
struct Capture {
    bytes: Vec<u8>,
    truncated: bool,
}

impl Capture {
    fn push(&mut self, data: &[u8]) {
        let room = KEEP_OUTPUT_BYTES.saturating_sub(self.bytes.len());
        self.bytes.extend_from_slice(&data[..data.len().min(room)]);
        self.truncated |= data.len() > room;
    }
}

async fn make_one(
    engine: &Engine,
    source: &Computer,
    name: &str,
    checkpoint: Option<String>,
    extras: &ops::ForkExtras,
    recipe_hash: Option<String>,
    progress: Progress<'_>,
) -> Result<Computer, CliError> {
    let mut params = CheckpointForkParams {
        computer_id: source.id.clone(),
        device_id: None,
        checkpoint_id: checkpoint,
        new_name: name.to_owned(),
        secrets: Vec::new(),
        quiet: None,
        recipe_hash,
    };
    ops::apply_fork_extras(&mut params, extras)?;
    engine
        .run_job(
            &format!("Forking {} into {name}", source.name),
            requests::checkpoints_fork(params),
            progress,
        )
        .await?;
    ops::start(engine, name, progress).await?;
    engine.computer(name).await
}

#[allow(clippy::too_many_arguments)]
async fn one(
    engine: &Engine,
    source: &Computer,
    name: &str,
    checkpoint: Option<String>,
    extras: &ops::ForkExtras,
    args: &ManyArgs,
    progress: Progress<'_>,
    output: Output<'_>,
) -> Value {
    let mut entry = json!({"name": name, "source": source.name});
    let fork = match make_one(engine, source, name, checkpoint, extras, None, progress).await {
        Ok(fork) => fork,
        Err(error) => {
            entry["error"] = json!(error.message);
            return entry;
        }
    };
    entry["id"] = json!(fork.id);
    entry["state"] = json!(fork.state);
    entry["kind"] = json!(fork.kind);
    entry["urls"] = json!(urls(engine, &fork).await);
    if args.command.is_empty() {
        return entry;
    }
    let (mut out, mut err) = (Capture::default(), Capture::default());
    let ran = exec_streaming(
        engine,
        &fork,
        &args.command,
        args.cwd.as_deref(),
        progress,
        |is_err, bytes| {
            if is_err {
                err.push(bytes);
            } else {
                out.push(bytes);
            }
            output(name, is_err, bytes);
        },
    )
    .await;
    match ran {
        Ok(ran) => {
            entry["exitCode"] = json!(ran.exit_code);
            entry["signal"] = json!(ran.signal);
        }
        Err(error) => entry["error"] = json!(error.message),
    }
    entry["stdout"] = json!(String::from_utf8_lossy(&out.bytes));
    entry["stderr"] = json!(String::from_utf8_lossy(&err.bytes));
    entry["outputTruncated"] = json!(out.truncated || err.truncated);
    entry
}

/// Makes `count` forks of one checkpoint at the same time, starts them and, when a command is
/// given, runs it in each. A fork that fails reports `error`; the others are still made.
/// The result has `forks`, `checkpoint` (the one all of them came from) and `ok`.
pub async fn fork_many(
    engine: &Engine,
    args: &ManyArgs,
    settings: Option<&Settings>,
    progress: Progress<'_>,
    output: Output<'_>,
) -> Result<Value, CliError> {
    if args.count == 0 || args.count > MAX_FORKS {
        return Err(CliError::new(format!(
            "-n takes 1 to {MAX_FORKS}; {} was given.",
            args.count
        )));
    }
    Settings::check_fork_secrets(settings, &args.fork.secrets)?;
    let all = engine.computers().await?;
    let source = find(&all, &args.fork.source)?.clone();
    let names = fork_names(
        &source.name,
        args.fork.name.as_deref(),
        args.count,
        &all,
        ops::short_id,
    )?;
    check_capacity(
        &source,
        &all,
        args.count,
        settings.and_then(Settings::fork_ram_budget_bytes),
    )?;
    let mut checkpoint = args.fork.checkpoint.clone();
    if checkpoint.is_none() {
        checkpoint = ops::warm_checkpoint(engine, &source).await;
    }
    if checkpoint.is_none() && args.count > 1 {
        let made = ops::checkpoint(
            engine,
            &source.name,
            Some(&format!("silo-fork-{}", ops::short_id())),
            Some(CheckpointKind::Manual),
            progress,
        )
        .await?;
        checkpoint = made["id"].as_str().map(str::to_owned);
    }
    let extras = ops::fork_extras(args.fork.recipe.as_ref(), &args.fork.secrets);
    let forks = futures::future::join_all(names.iter().map(|name| {
        one(
            engine,
            &source,
            name,
            checkpoint.clone(),
            &extras,
            args,
            progress,
            output,
        )
    }))
    .await;
    let ok = forks
        .iter()
        .all(|fork| fork.get("error").is_none() && fork["exitCode"].as_i64().unwrap_or(0) == 0);
    Ok(json!({
        "source": source.name,
        "checkpoint": checkpoint,
        "forks": forks,
        "ok": ok,
    }))
}

/// The exit code for the set: the first fork's failure, else zero.
pub fn exit_code(result: &Value) -> i32 {
    for fork in result["forks"].as_array().into_iter().flatten() {
        if fork.get("error").is_some() {
            return 1;
        }
        let code = fork["exitCode"].as_i64().unwrap_or(0) as i32;
        if code != 0 {
            return code;
        }
    }
    0
}

/// Writes lines of several forks' output with the fork's name in front, keeping a partial
/// line until its end arrives.
#[derive(Default)]
pub struct Prefixer {
    partial: std::collections::HashMap<(String, bool), Vec<u8>>,
}

impl Prefixer {
    /// The complete lines in `bytes` (and what was held back), each as `name | line`.
    pub fn push(&mut self, name: &str, is_err: bool, bytes: &[u8]) -> Vec<u8> {
        let held = self.partial.entry((name.to_owned(), is_err)).or_default();
        held.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some(end) = held.iter().position(|byte| *byte == b'\n') {
            out.extend_from_slice(format!("[{name}] ").as_bytes());
            out.extend(held.drain(..=end));
        }
        out
    }

    /// What is left when the commands have ended: unfinished lines, ended with a newline.
    pub fn finish(&mut self) -> Vec<(bool, Vec<u8>)> {
        let mut out = Vec::new();
        for ((name, is_err), held) in self.partial.drain() {
            if !held.is_empty() {
                let mut line = format!("[{name}] ").into_bytes();
                line.extend(held);
                line.push(b'\n');
                out.push((is_err, line));
            }
        }
        out
    }
}
