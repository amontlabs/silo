//! Display session and viewer control methods.

use super::common::ComputerTargetParams;
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, fmt};

pub type DisplayShowParams = ComputerTargetParams;
pub type DisplayCloseParams = ComputerTargetParams;

wire_struct! {
    ViewerConnectionParams {
        connection_id: String,
    }
}

wire_struct! {
    ViewerAudioSetParams {
        connection_id: String,
        muted: bool,
        active: bool,
        revision: u64,
    }
}

wire_struct! {
    ViewerOpenResult {
        connection_id: String,
        url: String,
        cookie: ViewerCookie,
        init_script: String,
        name: String,
    }
}

wire_struct! {
    ViewerCookie {
        name: String,
        value: String,
        domain: String,
        path: String,
        http_only: bool,
    }
}

wire_struct! {
    ViewerSoundCheckResult {
        sound: bool,
    }
}

wire_struct! {
    ViewerPageEvent {
        connection_id: String,
        script: String,
    }
}

wire_struct! {
    NativeDisplayAttachParams {
        computer_id: String,
    }
}

/// JSON messages used by the native display stream. Pixel data itself is
/// transferred out of band through the shared-memory path.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(tag = "t", rename_all = "lowercase", rename_all_fields = "camelCase")]
pub enum NativeDisplayMessage {
    Hello {
        sandbox: String,
    },
    Configure {
        scanout: u32,
        width: u32,
        height: u32,
        format: String,
        path: String,
        slots: u32,
    },
    Frame {
        scanout: u32,
        slot: u32,
        seq: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rect: Option<[u32; 4]>,
    },
    Disable {
        scanout: u32,
    },
    Key {
        code: u32,
        down: bool,
    },
    Abs {
        x: DisplayAxis,
        y: DisplayAxis,
    },
    Btn {
        code: u32,
        down: bool,
    },
    Rel {
        code: u32,
        value: i32,
    },
}

/// Absolute input coordinate accepted by the native display protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct DisplayAxis(u16);

impl DisplayAxis {
    pub fn new(value: u32) -> Result<Self, DisplayAxisError> {
        if value <= 32_767 {
            Ok(Self(value as u16))
        } else {
            Err(DisplayAxisError)
        }
    }

    pub fn value(self) -> u16 {
        self.0
    }
}

impl<'de> Deserialize<'de> for DisplayAxis {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = u32::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for DisplayAxis {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("DisplayAxis")
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "integer",
            "minimum": 0,
            "maximum": 32767
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayAxisError;

impl fmt::Display for DisplayAxisError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("display axis must be in 0..=32767")
    }
}

impl std::error::Error for DisplayAxisError {}
