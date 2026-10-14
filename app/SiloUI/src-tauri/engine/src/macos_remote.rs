//! macOS computers on another device. The owner (the Mac that runs the virtual machines)
//! serves the `macos.*` bridge methods below from its running Silo; the controlling device
//! calls them through the same bridge as every other remote operation.
//!
//! | Method | Access | Params | Result |
//! | --- | --- | --- | --- |
//! | `macos.snapshot` | read | none | the owner's macOS computers, as `read_macos_computers` |
//! | `macos.create` | change | `request` (name, cpus, memoryGiB, diskGiB) | the new computer's row |
//! | `macos.action` | change | `computerId`, `action` | the owner's macOS computers afterwards |
//! | `macos.display.connect` | read | `computerId` | `username`, `password`, `width`, `height` |
//! | `macos.display.resize` | read | `computerId`, `widthPx`, `heightPx` | the size the display took |
//! | `macos.display.stream` | stream | `computerId` | raw bytes to the guest's Screen Sharing port |
//!
//! Resize is idempotent and replaces the previous size, so it is classified as a read: a
//! viewer sends it while its window is dragged, and a replay record per call would only be
//! noise. Starting a computer through `macos.action` never opens a window on the owner.
use crate::bridge_error::BridgeError;
use crate::macos_computers_engine::{self as macos_computers, Action, CreateRequest, MacosContext};
use serde_json::{json, Value};

/// The stream method that relays a computer's Screen Sharing connection.
pub const DISPLAY_STREAM: &str = "macos.display.stream";

fn invalid(what: &str) -> String {
    format!("Invalid macOS computer request: {what}.")
}

/// The owner's computer id in `params`: a UUID, as every macOS computer's id is.
pub(crate) fn computer_id(params: &Value) -> Result<&str, String> {
    params["computerId"]
        .as_str()
        .filter(|id| uuid::Uuid::parse_str(id).is_ok())
        .ok_or_else(|| invalid("missing computer identity"))
}

pub fn parse_create(params: &Value) -> Result<CreateRequest, String> {
    serde_json::from_value(params["request"].clone())
        .map_err(|_| invalid("the computer details are not valid"))
}

pub fn parse_action(params: &Value) -> Result<(&str, Action), String> {
    let id = computer_id(params)?;
    let action = serde_json::from_value(params["action"].clone())
        .map_err(|_| invalid("unsupported action"))?;
    Ok((id, action))
}

pub(crate) fn parse_resize(params: &Value) -> Result<(&str, u64, u64), String> {
    let id = computer_id(params)?;
    match (params["widthPx"].as_u64(), params["heightPx"].as_u64()) {
        (Some(width), Some(height)) => Ok((id, width, height)),
        _ => Err(invalid("the display size is not valid")),
    }
}

/// Runs one `macos.*` request on this device.
pub(crate) fn dispatch(
    context: &MacosContext,
    method: &str,
    params: &Value,
) -> Result<Value, BridgeError> {
    let to_value = |value: Result<Value, serde_json::Error>| {
        value.map_err(|error| BridgeError::from(error.to_string()))
    };
    match method {
        "macos.snapshot" => to_value(serde_json::to_value(
            macos_computers::state_for_remote(context).map_err(BridgeError::from)?,
        )),
        "macos.create" => {
            let request = parse_create(params)?;
            to_value(serde_json::to_value(
                macos_computers::create_for_remote(context, request).map_err(BridgeError::from)?,
            ))
        }
        "macos.action" => {
            let (id, action) = parse_action(params)?;
            macos_computers::action_for_remote(context, id, action).map_err(BridgeError::from)?;
            to_value(serde_json::to_value(
                macos_computers::state_for_remote(context).map_err(BridgeError::from)?,
            ))
        }
        "macos.display.connect" => {
            let session = macos_computers::display_session(context, computer_id(params)?)
                .map_err(BridgeError::from)?;
            to_value(serde_json::to_value(session))
        }
        "macos.display.resize" => {
            let (id, width, height) = parse_resize(params)?;
            let (width, height) = macos_computers::resize_display(context, id, width, height)
                .map_err(BridgeError::from)?;
            Ok(json!({"widthPx": width, "heightPx": height}))
        }
        _ => Err(BridgeError::unsupported()),
    }
}

/// Opens the guest's Screen Sharing connection a `macos.display.stream` relays.
pub(crate) fn open_display_stream(
    context: &MacosContext,
    params: &Value,
) -> Result<std::net::TcpStream, String> {
    macos_computers::display_stream(context, computer_id(params)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "11111111-1111-4111-8111-111111111111";

    #[test]
    fn a_computer_identity_must_be_a_uuid() {
        assert_eq!(computer_id(&json!({"computerId": ID})), Ok(ID));
        for bad in [
            json!({}),
            json!({"computerId": ""}),
            json!({"computerId": "../x"}),
            json!({"computerId": "name"}),
            json!({"computerId": 7}),
        ] {
            assert!(computer_id(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn creation_details_are_checked_before_the_owner_sees_them() {
        let ok = json!({"request": {"name": "mac", "cpus": 4, "memoryGiB": 8, "diskGiB": 64}});
        let request = parse_create(&ok).unwrap();
        assert_eq!((request.name.as_str(), request.cpus), ("mac", 4));
        for bad in [
            json!({}),
            json!({"request": {"name": "mac"}}),
            json!({"request": {"name": 1, "cpus": 4, "memoryGiB": 8, "diskGiB": 64}}),
            json!({"request": {"name": "m", "cpus": -1, "memoryGiB": 8, "diskGiB": 64}}),
        ] {
            assert!(parse_create(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn only_the_known_actions_are_accepted() {
        for (name, expected) in [
            ("start", Action::Start),
            ("stop", Action::Stop),
            ("force-stop", Action::ForceStop),
            ("delete", Action::Delete),
            ("setup", Action::Setup),
        ] {
            let params = json!({"computerId": ID, "action": name});
            let (id, action) = parse_action(&params).unwrap();
            assert_eq!((id, action), (ID, expected));
        }
        for bad in ["restart", "dismiss-error", "", "Start"] {
            assert!(parse_action(&json!({"computerId": ID, "action": bad})).is_err());
        }
        assert!(parse_action(&json!({"computerId": ID})).is_err());
        assert!(parse_action(&json!({"action": "start"})).is_err());
    }

    #[test]
    fn a_resize_needs_whole_pixel_sizes() {
        assert_eq!(
            parse_resize(&json!({"computerId": ID, "widthPx": 1920, "heightPx": 1080})).unwrap(),
            (ID, 1920, 1080)
        );
        for bad in [
            json!({"computerId": ID, "widthPx": 1920}),
            json!({"computerId": ID, "widthPx": -1, "heightPx": 10}),
            json!({"computerId": ID, "widthPx": 1.5, "heightPx": 10}),
            json!({"computerId": ID, "widthPx": "1920", "heightPx": 1080}),
            json!({"widthPx": 1920, "heightPx": 1080}),
        ] {
            assert!(parse_resize(&bad).is_err(), "{bad}");
        }
    }
}
