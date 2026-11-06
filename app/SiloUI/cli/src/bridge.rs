//! Drives a computer's desktop from the host through the Cua Driver that runs in it.
//!
//! Each call is one `guest.exec` as the working account that runs a small Python helper
//! (`BRIDGE`) in the computer. The helper starts the driver's MCP server the way agents in the
//! computer do (on Linux through `/usr/local/libexec/silo-cua-driver-mcp`, which finds the
//! live desktop session; on macOS `cua-driver mcp`), sends one `tools/call` or `tools/list`
//! over stdio and prints the result as one JSON line. A result too large for one `guest.exec`
//! reply (a screenshot) is left in a file the host reads in parts and removes.

use std::time::Duration;

use serde_json::{json, Value};

use crate::{
    engine::{Computer, Engine},
    error::CliError,
    ops::{self, ExecArgs},
};

/// The computer's driver launcher, by platform.
pub fn driver_argv(kind: &str) -> Vec<String> {
    match kind {
        "macos" => vec!["/usr/local/bin/cua-driver".into(), "mcp".into()],
        _ => vec!["/usr/local/libexec/silo-cua-driver-mcp".into()],
    }
}

/// Python run inside the computer: `python3 -c BRIDGE <timeout-seconds> <driver command...>`
/// with `{"method": "tools/call", "params": {...}}` on standard input.
pub const BRIDGE: &str = r#"
import json, os, select, subprocess, sys, time
limit = float(sys.argv[1]); command = sys.argv[2:]
request = json.loads(sys.stdin.read())
deadline = time.monotonic() + limit
def fail(code, message):
    sys.stderr.write(message + "\n"); sys.exit(code)
try:
    proc = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
except OSError as error:
    fail(2, "cannot start the driver: %s" % error)
buffer = b""
def send(message):
    proc.stdin.write((json.dumps(message) + "\n").encode()); proc.stdin.flush()
def receive(wanted):
    global buffer
    fd = proc.stdout.fileno()
    while True:
        while b"\n" in buffer:
            line, buffer = buffer.split(b"\n", 1)
            try:
                message = json.loads(line)
            except ValueError:
                continue
            if isinstance(message, dict) and message.get("id") == wanted:
                return message
        left = deadline - time.monotonic()
        if left <= 0:
            fail(3, "the driver did not answer in time")
        if not select.select([fd], [], [], left)[0]:
            fail(3, "the driver did not answer in time")
        chunk = os.read(fd, 1 << 20)
        if not chunk:
            fail(4, "the driver ended before it answered")
        buffer += chunk
send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "silo", "version": "1"}}})
receive(1)
send({"jsonrpc": "2.0", "method": "notifications/initialized"})
send({"jsonrpc": "2.0", "id": 2, "method": request["method"], "params": request.get("params") or {}})
answer = receive(2)
try:
    proc.stdin.close(); proc.wait(timeout=3)
except Exception:
    proc.kill()
if "error" in answer:
    fail(5, json.dumps(answer["error"]))
text = json.dumps(answer.get("result"))
if len(text) > 600000:
    path = "/tmp/silo-cu.bridge-%d" % os.getpid()
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w") as out:
        out.write(text)
    text = json.dumps({"silo_result_file": path, "bytes": len(text)})
print(text)
"#;

/// Reads `<length>` bytes of a file from `<offset>`: `python3 -c READER <path> <offset> <length>`.
const READER: &str = "import sys; f = open(sys.argv[1], 'rb'); f.seek(int(sys.argv[2])); sys.stdout.buffer.write(f.read(int(sys.argv[3])))";
const PART: u64 = 500_000;
const RESULT_FILE_PREFIX: &str = "/tmp/silo-cu.bridge-";
const MAX_RESULT_BYTES: u64 = 24_000_000;

fn python() -> &'static str {
    "python3"
}

fn guest_args(computer: &Computer, argv: Vec<String>, timeout_secs: u64) -> ExecArgs {
    ExecArgs {
        computer: computer.name.clone(),
        argv,
        cwd: None,
        timeout_secs,
    }
}

/// One MCP request to the driver in `computer`. Returns the MCP `result` object.
pub async fn driver_request(
    engine: &Engine,
    computer: &Computer,
    method: &str,
    params: Value,
    timeout_secs: u64,
) -> Result<Value, CliError> {
    let window = timeout_secs.clamp(5, ops::MAX_EXEC_SECS - 20);
    let mut argv = vec![
        python().into(),
        "-c".into(),
        BRIDGE.into(),
        window.to_string(),
    ];
    argv.extend(driver_argv(computer.kind));
    let input = json!({"method": method, "params": params})
        .to_string()
        .into_bytes();
    let output = ops::exec_with_input(
        engine,
        computer.clone(),
        &guest_args(computer, argv, window + 15),
        Some(input),
        Some(700_000),
    )
    .await?;
    if output.process_exit_code() != 0 {
        return Err(CliError::new(failure_text(&computer.name, &output)));
    }
    let line = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let mut result: Value = serde_json::from_str(&line)
        .map_err(|_| CliError::new("The driver in the computer returned something unreadable."))?;
    if let Some(path) = result["silo_result_file"].as_str().map(str::to_owned) {
        let bytes = result["bytes"].as_u64().unwrap_or(0);
        result = fetch_file(engine, computer, &path, bytes).await?;
    }
    Ok(result)
}

fn failure_text(name: &str, output: &ops::ExecOutput) -> String {
    let said = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let said = said.lines().last().unwrap_or("").to_owned();
    match output.process_exit_code() {
        2 => format!("{name} has no computer-use driver ({said}). Start it and wait for computer use to be ready (`silo status {name}`)."),
        3 => format!("{name}'s driver did not answer in time. The desktop may not be ready."),
        5 => format!("The driver in {name} refused the request: {said}"),
        127 => format!("{name} has no python3 to run the driver bridge."),
        code => format!("The driver bridge in {name} failed (exit {code}): {said}"),
    }
}

async fn fetch_file(
    engine: &Engine,
    computer: &Computer,
    path: &str,
    bytes: u64,
) -> Result<Value, CliError> {
    if !path.starts_with(RESULT_FILE_PREFIX) || path.contains("..") || bytes > MAX_RESULT_BYTES {
        return Err(CliError::new(
            "The driver returned a result that cannot be read.",
        ));
    }
    let mut data = Vec::new();
    let mut offset = 0;
    let read = async {
        while offset < bytes {
            let part = PART.min(bytes - offset);
            let argv = vec![
                python().into(),
                "-c".into(),
                READER.into(),
                path.to_owned(),
                offset.to_string(),
                part.to_string(),
            ];
            let out = ops::exec_with_input(
                engine,
                computer.clone(),
                &guest_args(computer, argv, 30),
                None,
                Some(700_000),
            )
            .await?;
            if out.process_exit_code() != 0 || out.stdout.is_empty() {
                return Err(CliError::new("Could not read the driver's result back."));
            }
            offset += out.stdout.len() as u64;
            data.extend(out.stdout);
        }
        Ok::<_, CliError>(())
    }
    .await;
    // Removed whether or not the read worked.
    let _ = ops::exec_with_input(
        engine,
        computer.clone(),
        &guest_args(
            computer,
            vec!["rm".into(), "-f".into(), path.to_owned()],
            15,
        ),
        None,
        None,
    )
    .await;
    read?;
    serde_json::from_slice(&data)
        .map_err(|_| CliError::new("The driver's result was damaged on the way back."))
}

/// `wait`: a pause on the host, capped.
pub async fn pause(seconds: f64) {
    tokio::time::sleep(Duration::from_secs_f64(seconds.clamp(0.0, 30.0))).await;
}
