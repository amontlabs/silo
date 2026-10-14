//! Clipboard operations for macOS computers and desktop viewers.

use serde::{Deserialize, Serialize};

use super::common::ComputerTargetParams;

pub type ClipboardReadParams = ComputerTargetParams;

/// The specification does not yet fix the exact fields of all clipboard
/// payloads. This tagged union keeps text and PNG bytes explicit.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ClipboardContent {
    Text { text: String },
    Image { mime: String, bytes: Vec<u8> },
    Empty,
    TooLarge,
}

pub type ClipboardReadResult = ClipboardContent;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ClipboardAction {
    Copy,
    Paste,
}

wire_struct! {
    ClipboardTransferParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        connection_id: Option<String>,
        action: ClipboardAction,
    }
}

wire_struct! {
    ClipboardTransferResult {
        action: ClipboardAction,
        status: ClipboardTransferStatus,
        content: Option<ClipboardTransferContent>,
        message: Option<String>,
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ClipboardTransferContent {
    Text,
    Image,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ClipboardTransferStatus {
    Pasted,
    Copied,
    DeviceEmpty,
    ComputerEmpty,
    TooLarge,
    NotConnected,
    Unsupported,
    Busy,
    Failed,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ClipboardWritableContent {
    Text { text: String },
    Image { mime: String, bytes: Vec<u8> },
}

wire_struct! {
    ClipboardWriteParams {
        computer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        content: ClipboardWritableContent,
    }
}

/// `clipboard.write` has no declared result shape in 30 §6; null is used.
pub type ClipboardWriteResult = ();
