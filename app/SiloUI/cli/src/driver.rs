//! One long-lived Cua Driver MCP session per computer, spoken through the engine's
//! `guest.exec.start`, `guest.exec.input` and `guest.exec.get`.
//!
//! The driver runs in the computer as the working account (Linux: the launcher
//! `/usr/local/libexec/silo-cua-driver-mcp`, which finds the live desktop session; macOS:
//! `/usr/local/bin/cua-driver mcp`). JSON-RPC lines go in through `guest.exec.input` and come
//! back through `guest.exec.get`. When the driver exits, the next request starts it again.

use std::time::{Duration, Instant};

use serde_json::{json, Value};
use silo_api::methods::{
    guest::{GuestExecGetParams, GuestExecInputParams, GuestExecStartParams},
    requests,
};

use crate::{
    engine::{Computer, Engine},
    error::CliError,
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
const STDERR_KEEP: usize = 4096;
/// The longest single JSON-RPC line read back (a base64 screenshot).
const MAX_LINE: usize = 24 * 1024 * 1024;

#[derive(Default)]
pub struct Session {
    job: Option<silo_api::JobId>,
    seen: u64,
    buffer: Vec<u8>,
    stderr: Vec<u8>,
    next_id: u64,
}

enum Failure {
    /// The driver process is gone; a new one may be started.
    Exited,
    Other(CliError),
}

impl From<CliError> for Failure {
    fn from(error: CliError) -> Self {
        Self::Other(error)
    }
}

impl Session {
    /// Sends one MCP request and returns its `result`. Starts the driver first when none runs,
    /// and once more when it turns out to have exited.
    pub async fn request(
        &mut self,
        engine: &Engine,
        computer: &Computer,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, CliError> {
        for attempt in 0..2 {
            let outcome = self
                .try_request(engine, computer, method, &params, timeout)
                .await;
            match outcome {
                Ok(result) => return Ok(result),
                Err(Failure::Exited) => {
                    let said = self.stderr_text();
                    self.job = None;
                    if attempt == 1 {
                        return Err(CliError::new(format!(
                            "The computer-use driver in {} keeps exiting{}. Check `silo status {}`.",
                            computer.name, said, computer.name
                        )));
                    }
                }
                Err(Failure::Other(error)) => return Err(error),
            }
        }
        unreachable!("the loop returns")
    }

    /// Stops the driver (its standard input closes), if one runs.
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
    }

    fn stderr_text(&self) -> String {
        let text = String::from_utf8_lossy(&self.stderr);
        let text = text.trim();
        if text.is_empty() {
            String::new()
        } else {
            format!(" ({})", text.lines().last().unwrap_or(""))
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
        self.call(engine, computer, method, params, timeout).await
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
        let init = json!({"protocolVersion": "2025-06-18", "capabilities": {},
                          "clientInfo": {"name": "silo", "version": "1"}});
        self.call(engine, computer, "initialize", &init, timeout)
            .await?;
        self.send(
            engine,
            &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        )
        .await?;
        Ok(())
    }

    async fn send(&mut self, engine: &Engine, message: &Value) -> Result<(), Failure> {
        let job = self.job.clone().ok_or(Failure::Exited)?;
        let mut line = message.to_string().into_bytes();
        line.push(b'\n');
        engine
            .call(requests::guest_exec_input(GuestExecInputParams {
                job_id: job,
                device_id: None,
                data: Some(line),
                close: None,
            }))
            .await
            .map_err(|_| Failure::Exited)?;
        Ok(())
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
            if Instant::now() >= deadline {
                return Err(CliError::new(format!(
                    "The driver in {} did not answer in {}s. The desktop may not be ready.",
                    computer.name,
                    timeout.as_secs()
                ))
                .into());
            }
            if !self.pull(engine).await? {
                tokio::time::sleep(POLL).await;
            }
        }
    }

    /// The reply with this id, once its line has arrived.
    fn take_message(&mut self, id: u64) -> Option<Value> {
        while let Some(end) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=end).collect();
            if let Ok(message) = serde_json::from_slice::<Value>(&line) {
                if message["id"] == id {
                    return Some(message);
                }
            }
        }
        None
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
        if first > self.seen + 1 && self.seen > 0 {
            return Err(
                CliError::new("Output of the computer-use driver was lost; try again.").into(),
            );
        }
        let mut received = false;
        for chunk in reply["output"].as_array().into_iter().flatten() {
            received = true;
            self.seen = self.seen.max(chunk["seq"].as_u64().unwrap_or(0));
            let bytes = chunk["data"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|byte| byte.as_u64().map(|byte| byte as u8));
            if chunk["stream"] == "stderr" {
                let room = STDERR_KEEP.saturating_sub(self.stderr.len());
                self.stderr.extend(bytes.take(room));
            } else {
                self.buffer.extend(bytes);
                if self.buffer.len() > MAX_LINE && !self.buffer.contains(&b'\n') {
                    self.buffer.clear();
                    return Err(
                        CliError::new("The driver sent an answer that is too large.").into(),
                    );
                }
            }
        }
        if reply["state"].as_str() != Some("running") && !received {
            return Err(Failure::Exited);
        }
        Ok(received)
    }
}
