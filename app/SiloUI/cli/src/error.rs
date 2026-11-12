//! The one error type the commands and the MCP tools share.

use std::fmt;

use silo_api::jobs::ErrorCode;
use silo_client::ClientError;

/// The process exit codes the commands use besides 0 (done) and 1 (failed).
pub mod exit {
    /// A merge, replace-main or undo that the engine refused; nothing changed.
    pub const REFUSED: i32 = 2;
    /// A merge that needs an answer for some items: run it again with `--choose`.
    pub const CHOICES_NEEDED: i32 = 3;
    /// The owner denied what the command asked for.
    pub const DENIED: i32 = 4;
    /// The owner has not answered yet; the command did not run.
    pub const WAITING: i32 = 5;
    /// The request ended without an answer: withdrawn, or expired.
    pub const ENDED: i32 = 6;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliError {
    pub message: String,
    /// The process exit code of a command that fails with this error.
    pub exit: i32,
    /// For an error that waits on the owner: the request to check later.
    pub approval_id: Option<String>,
    /// The engine answered that the owner must approve the call: the request's id and what
    /// it asks. Only a call that was not waited for carries it.
    pub approval_request: Option<(String, String)>,
}

impl CliError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit: 1,
            approval_id: None,
            approval_request: None,
        }
    }

    pub fn with_exit(mut self, exit: i32) -> Self {
        self.exit = exit;
        self
    }

    pub fn for_approval(mut self, id: impl Into<String>) -> Self {
        self.approval_id = Some(id.into());
        self
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CliError {}

impl From<ClientError> for CliError {
    fn from(error: ClientError) -> Self {
        let mut converted = Self::new(error.to_string());
        if let ClientError::Remote(remote) = &error {
            if remote.code == ErrorCode::ApprovalRequired {
                let asked = remote.details.as_ref();
                match asked.and_then(|details| details.approval_id.clone()) {
                    Some(id) => {
                        let summary = asked.map(|details| details.summary.clone());
                        converted.approval_request =
                            Some((id, summary.unwrap_or_else(|| remote.message.clone())));
                    }
                    None => {
                        converted.message = format!(
                            "{} The owner has to approve it, but the engine named no request to follow.",
                            remote.message
                        );
                    }
                }
            }
        }
        converted
    }
}

impl From<serde_json::Error> for CliError {
    fn from(error: serde_json::Error) -> Self {
        Self::new(format!("Unreadable engine reply: {error}"))
    }
}
