//! One long-lived Cua Driver MCP session per computer, spoken through the engine's
//! `guest.exec.start`, `guest.exec.input` and `guest.exec.get`.
//!
//! The driver runs in the computer as the working account (Linux: the launcher
//! `/usr/local/libexec/silo-cua-driver-mcp`, which finds the live desktop session; macOS:
//! `/usr/local/bin/cua-driver mcp`). JSON-RPC lines go in through `guest.exec.input` and come
//! back through `guest.exec.get`. Any failure ends the session (its standard input is closed
//! and its state cleared); the next request starts a new one. Only requests that change
//! nothing (`tools/list`, `get_desktop_state`) are repeated on a new session after the driver
//! exited. Screenshots never travel on the driver's standard output: the driver writes the PNG
//! to a temporary file in the computer, which is read back in parts and removed.
//!
//! A computer on another device is driven the same way: the `Engine` acting on that device
//! adds its `deviceId` to each of these calls, and the controller's engine sends them to the
//! owner, whose job ids the session follows.

use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use silo_api::methods::{
    guest::{GuestExecGetParams, GuestExecInputParams, GuestExecStartParams},
    requests,
};

use crate::{
    engine::{Computer, Engine},
    error::CliError,
    ops::{self, ExecArgs},
};

/// The command that starts the driver's MCP server in a computer.
pub fn driver_argv(kind: &str) -> Vec<String> {
    match kind {
        "macos" => vec!["/usr/local/bin/cua-driver".into(), "mcp".into()],
        _ => vec!["/usr/local/libexec/silo-cua-driver-mcp".into()],
    }
}

const RETAIN_OUTPUT_BYTES: u64 = 960 * 1024;
const POLL: Duration = Duration::from_millis(50);
/// The poll on a computer of another device, where every read is an SSH exchange.
const REMOTE_POLL: Duration = Duration::from_millis(250);

fn poll(engine: &Engine) -> Duration {
    if engine.device().is_some() {
        REMOTE_POLL
    } else {
        POLL
    }
}
const STDERR_KEEP: usize = 4096;
/// The longest JSON-RPC line read back; a screenshot never travels in one.
pub const MAX_LINE: usize = 256 * 1024;
const PART: u64 = 500_000;
const MAX_SCREENSHOT_BYTES: u64 = 16 * 1024 * 1024;
const SCREENSHOT_PREFIX: &str = "/tmp/silo-cu.shot-";

#[derive(Default)]
pub struct Session {
    job: Option<silo_api::JobId>,
    seen: u64,
    buffer: Vec<u8>,
    stderr: Vec<u8>,
    next_id: u64,
    shots: u64,
    /// Whether this driver has taken a screenshot, which is what it maps coordinates from.
    mapped: bool,
    /// Whether the request being handled has been written to the driver.
    sent: bool,
    /// The longest side the agent last asked a screenshot for; a restarted driver maps with it.
    last_side: Option<u32>,
}

/// Said in the answer of a click sent without the screenshot that maps its coordinates.
const UNMAPPED_NOTE: &str = "Silo could not take the screenshot that maps coordinates before this call, so it was sent as it was. Take a screenshot and check the screen.";

fn with_note(mut result: Value, note: &str) -> Value {
    if let Some(content) = result.get_mut("content").and_then(Value::as_array_mut) {
        content.push(json!({"type": "text", "text": note}));
    }
    result
}

enum Failure {
    /// The driver process is gone.
    Exited,
    Other(CliError),
}

impl From<CliError> for Failure {
    fn from(error: CliError) -> Self {
        Self::Other(error)
    }
}

/// Tools whose coordinates the driver maps from its latest screenshot back to the screen; a
/// driver that has taken none reads them as screen pixels.
fn needs_mapping(method: &str, params: &Value) -> bool {
    method == "tools/call"
        && matches!(
            params["name"].as_str(),
            Some("click" | "scroll" | "move_cursor" | "drag")
        )
}

/// Requests that change nothing, so repeating one on a new driver is harmless.
fn repeatable(method: &str, params: &Value) -> bool {
    method == "tools/list" || (method == "tools/call" && params["name"] == "get_desktop_state")
}

impl Session {
    /// Sends one MCP request and returns its `result`. Starts the driver first when none runs.
    /// A failure closes the session.
    pub async fn request(
        &mut self,
        engine: &Engine,
        computer: &Computer,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, CliError> {
        let mut attempt = 0;
        loop {
            self.sent = false;
            let outcome = self
                .try_request(engine, computer, method, &params, timeout)
                .await;
            match outcome {
                Ok(result) => return Ok(result),
                Err(Failure::Exited) => {
                    let said = self.stderr_text();
                    let sent = self.sent;
                    self.close(engine).await;
                    attempt += 1;
                    if sent && !repeatable(method, &params) {
                        return Err(CliError::new(format!(
                            "The computer-use driver in {} exited while handling the request{said}; it may or may not have been carried out. Look at the screen before trying again.",
                            computer.name
                        )));
                    }
                    if attempt >= 2 {
                        return Err(CliError::new(format!(
                            "The computer-use driver in {} keeps exiting{said}. Check `silo status {}`.",
                            computer.name, computer.name
                        )));
                    }
                }
                Err(Failure::Other(error)) => {
                    self.close(engine).await;
                    return Err(error);
                }
            }
        }
    }

    /// A screenshot of the desktop as an MCP result with the image in it. The driver scales
    /// it to `max_side` and writes it to a temporary file in the computer, which is read in
    /// parts and removed.
    pub async fn screenshot(
        &mut self,
        engine: &Engine,
        computer: &Computer,
        max_side: u32,
        timeout: Duration,
    ) -> Result<Value, CliError> {
        self.shots += 1;
        self.last_side = Some(max_side);
        let path = format!(
            "{SCREENSHOT_PREFIX}{}-{}.png",
            std::process::id(),
            self.shots
        );
        let result = self
            .request(
                engine,
                computer,
                "tools/call",
                json!({"name": "get_desktop_state",
                       "arguments": {"max_image_dimension": max_side, "screenshot_out_file": path}}),
                timeout,
            )
            .await?;
        let image = read_file(engine, computer, &path).await;
        remove_file(engine, computer, &path).await;
        let image = image?;
        let mut content: Vec<Value> = result["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|item| item["type"] == "text")
            .cloned()
            .collect();
        content.push(
            json!({"type": "image", "mimeType": "image/png", "data": STANDARD.encode(image)}),
        );
        Ok(
            json!({"content": content, "structuredContent": result["structuredContent"],
                  "isError": result["isError"]}),
        )
    }

    /// Ends the driver: closes its standard input and forgets the session.
    pub async fn close(&mut self, engine: &Engine) {
        if let Some(job) = self.job.take() {
            let _ = engine
                .call(requests::guest_exec_input(GuestExecInputParams {
                    job_id: job,
                    device_id: None,
                    data: None,
                    close: Some(true),
                }))
                .await;
        }
        self.seen = 0;
        self.buffer.clear();
        self.next_id = 0;
        self.mapped = false;
    }

    fn stderr_text(&self) -> String {
        let text = String::from_utf8_lossy(&self.stderr);
        match text.trim().lines().last() {
            Some(line) if !line.is_empty() => format!(" ({line})"),
            _ => String::new(),
        }
    }

    async fn try_request(
        &mut self,
        engine: &Engine,
        computer: &Computer,
        method: &str,
        params: &Value,
        timeout: Duration,
    ) -> Result<Value, Failure> {
        if self.job.is_none() {
            self.start(engine, computer, timeout).await?;
        }
        let mut unmapped = false;
        if needs_mapping(method, params) && !self.mapped {
            // The first coordinate call before any screenshot: take one now, so coordinates
            // read off the screenshots the agent sees map the same way as later ones.
            self.shots += 1;
            let path = format!(
                "{SCREENSHOT_PREFIX}{}-{}.png",
                std::process::id(),
                self.shots
            );
            let side = self.last_side.unwrap_or(crate::usetools::DEFAULT_MAX_SIDE);
            let mapping = json!({"name": "get_desktop_state",
                "arguments": {"max_image_dimension": side, "screenshot_out_file": path}});
            let taken = self
                .call(engine, computer, "tools/call", &mapping, timeout)
                .await;
            remove_file(engine, computer, &path).await;
            match taken {
                Ok(_) => self.mapped = true,
                // The driver is gone: the caller starts it again. Anything else leaves the click
                // to be sent unmapped, with a note in its answer.
                Err(Failure::Exited) => return Err(Failure::Exited),
                Err(Failure::Other(_)) => unmapped = true,
            }
        }
        self.sent = false;
        let result = self.call(engine, computer, method, params, timeout).await?;
        if method == "tools/call" && params["name"] == "get_desktop_state" {
            self.mapped = true;
            if let Some(side) = params["arguments"]["max_image_dimension"].as_u64() {
                self.last_side = u32::try_from(side).ok();
            }
        }
        Ok(if unmapped {
            with_note(result, UNMAPPED_NOTE)
        } else {
            result
        })
    }

    async fn start(
        &mut self,
        engine: &Engine,
        computer: &Computer,
        timeout: Duration,
    ) -> Result<(), Failure> {
        let started = engine
            .call(requests::guest_exec_start(GuestExecStartParams {
                computer_id: computer.id.clone(),
                device_id: None,
                argv: driver_argv(computer.kind),
                cwd: None,
                env: None,
                as_user: None,
                stdin: None,
                stdin_open: true,
                retain_output_bytes: RETAIN_OUTPUT_BYTES,
            }))
            .await?;
        let job = started["jobId"]
            .as_str()
            .ok_or_else(|| CliError::new("The engine did not start the computer-use driver."))?;
        self.job = Some(
            silo_api::JobId::new(job)
                .map_err(|_| CliError::new("The engine sent an invalid job id."))?,
        );
        self.seen = 0;
        self.buffer.clear();
        self.stderr.clear();
        self.next_id = 0;
        self.mapped = false;
        let init = json!({"protocolVersion": "2025-06-18", "capabilities": {},
                          "clientInfo": {"name": "silo", "version": "1"}});
        self.call(engine, computer, "initialize", &init, timeout)
            .await?;
        self.send(
            engine,
            &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        )
        .await
    }

    async fn send(&mut self, engine: &Engine, message: &Value) -> Result<(), Failure> {
        let job = self.job.clone().ok_or(Failure::Exited)?;
        let mut line = message.to_string().into_bytes();
        line.push(b'\n');
        let written = engine
            .call(requests::guest_exec_input(GuestExecInputParams {
                job_id: job.clone(),
                device_id: None,
                data: Some(line),
                close: None,
            }))
            .await;
        match written {
            Ok(_) => Ok(()),
            Err(error) => {
                // A refused write is an exited driver only when the job says so.
                let state = engine
                    .call(requests::guest_exec_get(GuestExecGetParams {
                        job_id: job,
                        device_id: None,
                        since_seq: Some(self.seen),
                    }))
                    .await;
                match state {
                    Ok(reply) if reply["state"].as_str() != Some("running") => Err(Failure::Exited),
                    _ => Err(Failure::Other(error)),
                }
            }
        }
    }

    async fn call(
        &mut self,
        engine: &Engine,
        computer: &Computer,
        method: &str,
        params: &Value,
        timeout: Duration,
    ) -> Result<Value, Failure> {
        self.next_id += 1;
        let id = self.next_id;
        self.send(
            engine,
            &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
        )
        .await?;
        self.sent = true;
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(message) = self.take_message(id) {
                if let Some(error) = message.get("error") {
                    return Err(CliError::new(format!(
                        "The driver in {} refused the request: {}",
                        computer.name, error
                    ))
                    .into());
                }
                return Ok(message["result"].clone());
            }
            if self.buffer.len() > MAX_LINE {
                return Err(CliError::new("The driver sent an answer that is too large.").into());
            }
            if Instant::now() >= deadline {
                return Err(CliError::new(format!(
                    "The driver in {} did not answer in {}s. The desktop may not be ready.",
                    computer.name,
                    timeout.as_secs()
                ))
                .into());
            }
            if !self.pull(engine).await? {
                tokio::time::sleep(poll(engine)).await;
            }
        }
    }

    /// The reply with this id, once its line has arrived. Other lines are dropped.
    fn take_message(&mut self, id: u64) -> Option<Value> {
        let mut start = 0;
        let mut found = None;
        while let Some(offset) = self.buffer[start..].iter().position(|byte| *byte == b'\n') {
            let end = start + offset;
            if let Ok(message) = serde_json::from_slice::<Value>(&self.buffer[start..end]) {
                let reply = message.get("method").is_none()
                    && (message.get("result").is_some() || message.get("error").is_some());
                if reply && message["id"] == id {
                    found = Some(message);
                }
            }
            start = end + 1;
            if found.is_some() {
                break;
            }
        }
        self.buffer.drain(..start);
        found
    }

    /// Reads what the driver wrote since last time; false when there was nothing.
    async fn pull(&mut self, engine: &Engine) -> Result<bool, Failure> {
        let job = self.job.clone().ok_or(Failure::Exited)?;
        let reply = engine
            .call(requests::guest_exec_get(GuestExecGetParams {
                job_id: job,
                device_id: None,
                since_seq: Some(self.seen),
            }))
            .await?;
        let first = reply["firstRetainedSeq"].as_u64().unwrap_or(0);
        if first > self.seen + 1 {
            return Err(
                CliError::new("Output of the computer-use driver was lost; try again.").into(),
            );
        }
        let mut received = false;
        for chunk in reply["output"].as_array().into_iter().flatten() {
            received = true;
            self.seen = self.seen.max(chunk["seq"].as_u64().unwrap_or(0));
            let bytes: Vec<u8> = chunk["data"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|byte| byte.as_u64().map(|byte| byte as u8))
                .collect();
            if chunk["stream"] == "stderr" {
                self.stderr.extend(bytes);
                if self.stderr.len() > STDERR_KEEP {
                    let excess = self.stderr.len() - STDERR_KEEP;
                    self.stderr.drain(..excess);
                }
            } else {
                self.buffer.extend(bytes);
            }
        }
        if reply["state"].as_str() != Some("running") && !received {
            return Err(Failure::Exited);
        }
        Ok(received)
    }
}

fn guest(computer: &Computer, argv: Vec<String>) -> ExecArgs {
    ExecArgs {
        computer: computer.name.clone(),
        argv,
        cwd: None,
        timeout_secs: 30,
    }
}

fn sh(computer: &Computer, script: &str, args: &[String]) -> ExecArgs {
    let mut argv = vec!["sh".to_owned(), "-c".into(), script.to_owned(), "sh".into()];
    argv.extend(args.iter().cloned());
    guest(computer, argv)
}

/// A file the driver wrote in the computer, read in parts of at most `PART` bytes.
async fn read_file(engine: &Engine, computer: &Computer, path: &str) -> Result<Vec<u8>, CliError> {
    if !path.starts_with(SCREENSHOT_PREFIX) {
        return Err(CliError::new("Refusing to read that file."));
    }
    let size = ops::exec_with_input(
        engine,
        computer.clone(),
        &sh(computer, r#"wc -c < "$1""#, &[path.to_owned()]),
        None,
        None,
    )
    .await?;
    let size: u64 = String::from_utf8_lossy(&size.stdout)
        .trim()
        .parse()
        .map_err(|_| CliError::new("The driver did not leave a screenshot."))?;
    if size == 0 || size > MAX_SCREENSHOT_BYTES {
        return Err(CliError::new("The screenshot has an unusable size."));
    }
    let mut data = Vec::with_capacity(size as usize);
    while (data.len() as u64) < size {
        let args = [
            (data.len() + 1).to_string(),
            path.to_owned(),
            PART.to_string(),
        ];
        let part = ops::exec_with_input(
            engine,
            computer.clone(),
            &sh(computer, r#"tail -c +"$1" "$2" | head -c "$3""#, &args),
            None,
            Some(700_000),
        )
        .await?;
        if part.process_exit_code() != 0 || part.stdout.is_empty() {
            return Err(CliError::new("Could not read the screenshot back."));
        }
        data.extend(part.stdout);
    }
    data.truncate(size as usize);
    Ok(data)
}

async fn remove_file(engine: &Engine, computer: &Computer, path: &str) {
    if path.starts_with(SCREENSHOT_PREFIX) {
        let _ = ops::exec_with_input(
            engine,
            computer.clone(),
            &guest(computer, vec!["rm".into(), "-f".into(), path.to_owned()]),
            None,
            None,
        )
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_is_added_to_the_text_of_a_result() {
        let result = with_note(json!({"content": [{"type": "text", "text": "ok"}]}), "note");
        assert_eq!(result["content"][1]["text"], "note");
        assert_eq!(with_note(json!({}), "note"), json!({}));
    }

    #[test]
    fn only_coordinate_tools_need_a_screenshot_first() {
        for tool in ["click", "scroll", "move_cursor", "drag"] {
            assert!(
                needs_mapping("tools/call", &json!({"name": tool})),
                "{tool}"
            );
        }
        for tool in ["type_text", "press_key", "hotkey", "get_desktop_state"] {
            assert!(
                !needs_mapping("tools/call", &json!({"name": tool})),
                "{tool}"
            );
        }
        assert!(!needs_mapping("tools/list", &json!({})));
        assert!(!Session::default().mapped);
    }
}
