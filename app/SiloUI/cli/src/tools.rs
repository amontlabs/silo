//! `silo tools enable|disable|status <computer>`: Silo's tools in a computer. The engine opens
//! the computer's channel, writes its address and token into the computer itself, and puts the
//! `silo` program there and registers its MCP server with the agents (`guest_components`); this
//! command can also name a Linux `silo` for the engine to send instead of the one it ships
//! (`--binary`, for development).

use std::path::Path;

use serde_json::Value;
use silo_api::methods::{
    guest_client::{GuestClientComputerParams, GuestClientEnableParams},
    requests,
};

use crate::{
    engine::{Engine, Progress},
    error::CliError,
};

/// Turns Silo's tools on in `computer`. With `binary`, the engine sends that `silo` program into
/// the running Linux computer as `/usr/local/bin/silo` (checked there by its SHA-256) instead of
/// the program it ships; the computer's next start puts the shipped one back.
pub async fn enable(
    engine: &Engine,
    computer: &str,
    binary: Option<&Path>,
    _progress: Progress<'_>,
) -> Result<Value, CliError> {
    let target = engine.computer(computer).await?;
    let binary_path = match binary {
        Some(binary) => {
            if target.kind == "macos" {
                return Err(CliError::new(
                    "--binary sends a Linux program; a macOS computer uses the device's own silo.",
                ));
            }
            if target.state != "running" {
                return Err(CliError::new(format!(
                    "Start {computer} first; the program is copied into it."
                )));
            }
            let path = binary
                .canonicalize()
                .map_err(|error| CliError::new(format!("--binary cannot be read: {error}")))?;
            Some(path.to_string_lossy().into_owned())
        }
        None => None,
    };
    let mut status = engine
        .call(requests::guest_client_enable(GuestClientEnableParams {
            computer_id: target.id,
            binary_path,
        }))
        .await?;
    status["computer"] = Value::String(target.name);
    Ok(status)
}

pub async fn disable(engine: &Engine, computer: &str) -> Result<Value, CliError> {
    let target = engine.computer(computer).await?;
    let mut status = engine
        .call(requests::guest_client_revoke(GuestClientComputerParams {
            computer_id: target.id,
        }))
        .await?;
    status["computer"] = Value::String(target.name);
    Ok(status)
}

pub async fn status(engine: &Engine, computer: &str) -> Result<Value, CliError> {
    let target = engine.computer(computer).await?;
    let mut status = engine
        .call(requests::guest_client_status(GuestClientComputerParams {
            computer_id: target.id,
        }))
        .await?;
    status["computer"] = Value::String(target.name);
    Ok(status)
}

/// One line about a computer's tools.
pub fn render(status: &Value) -> String {
    let name = status["computer"]
        .as_str()
        .or(status["computerId"].as_str())
        .unwrap_or_default();
    if status["enabled"] != true {
        return format!("{name}: Silo's tools are off");
    }
    let port = status["port"].as_u64().unwrap_or_default();
    let reach = match status["network"].as_str() {
        Some("applied") => "it can reach them now",
        Some("nextStart") => "it can reach them once it starts",
        Some("nextRestart") => "restart it so it can reach them",
        _ => "its network is not known",
    };
    let file = if status["written"] == true {
        "; /etc/silo/endpoint.json was written"
    } else {
        ""
    };
    let mut text = format!("{name}: Silo's tools are on (port {port}); {reach}{file}");
    for component in status["components"].as_array().into_iter().flatten() {
        let id = component["id"].as_str().unwrap_or("?");
        let state = component["state"].as_str().unwrap_or("?");
        text.push_str(&format!("\n  {id}: {state}"));
        if let Some(detail) = component["detail"].as_str() {
            text.push_str(&format!(" ({detail})"));
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::render;

    #[test]
    fn the_status_line_is_followed_by_what_silo_keeps_in_the_computer() {
        let text = render(&json!({
            "computer": "main", "enabled": true, "port": 41000, "network": "applied", "written": false,
            "components": [
                {"id": "silo", "version": 1, "state": "ok", "at": 5},
                {"id": "silo-mcp", "version": 1, "state": "failed", "detail": "could not be registered with Codex", "at": 5}
            ]
        }));
        assert_eq!(
            text,
            "main: Silo's tools are on (port 41000); it can reach them now\n  silo: ok\n  silo-mcp: failed (could not be registered with Codex)"
        );
        assert_eq!(
            render(&json!({"computer": "main", "enabled": false})),
            "main: Silo's tools are off"
        );
    }
}
