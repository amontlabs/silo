//! Helpers shared by the commands that forward the access, files, transfer, export and
//! Connections domains: device scoping, computer name/id translation and the two client-side
//! launchers (browser, terminal) that open what the engine returned.

use serde_json::{json, Map, Value};

use super::{EngineClient, EngineError};

pub(crate) fn not_found(message: &str) -> EngineError {
    EngineError {
        code: "not_found".to_owned(),
        message: message.to_owned(),
    }
}

/// `params` with `deviceId` set when the command addresses a connected device.
pub(crate) fn with_device(mut params: Value, device_id: Option<&str>) -> Value {
    if let (Some(device_id), Some(object)) = (device_id, params.as_object_mut()) {
        object.insert("deviceId".to_owned(), Value::String(device_id.to_owned()));
    }
    params
}

/// The computers of one engine, as `computers.list` names them: `(id, display name)`.
pub(crate) struct Roster(Vec<(String, String)>);

impl Roster {
    pub(crate) fn from_listing(listing: &Value) -> Self {
        let linux = listing
            .pointer("/linux/computers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|computer| computer.get("configuration"));
        let macos = listing
            .pointer("/macos/computers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten();
        Self(
            linux
                .chain(macos)
                .filter_map(|computer| {
                    Some((
                        computer.get("id")?.as_str()?.to_owned(),
                        computer.get("name")?.as_str()?.to_owned(),
                    ))
                })
                .collect(),
        )
    }

    /// The id of the computer called `name` (an id is accepted as itself).
    pub(crate) fn id_of(&self, name: &str) -> Result<String, EngineError> {
        self.0
            .iter()
            .find(|(id, display)| display == name || id == name)
            .map(|(id, _)| id.clone())
            .ok_or_else(|| not_found("This computer no longer exists on this device."))
    }

    /// The display name of `id`; a computer the roster no longer lists keeps its id.
    pub(crate) fn name_of(&self, id: &str) -> String {
        self.0
            .iter()
            .find(|(candidate, _)| candidate == id)
            .map_or_else(|| id.to_owned(), |(_, name)| name.clone())
    }
}

pub(crate) async fn roster(
    engine: &EngineClient,
    device_id: Option<&str>,
) -> Result<Roster, EngineError> {
    let listing = engine
        .call("computers.list", with_device(json!({}), device_id))
        .await?;
    Ok(Roster::from_listing(&listing))
}

/// What the UI's `computer` argument names: a computer of this device by display name, or
/// one of a connected device as `silo-remote:<deviceId>:<computerId>`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Local(String),
    Remote {
        device_id: String,
        computer_id: String,
    },
}

pub(crate) fn parse_target(value: &str) -> Result<Target, EngineError> {
    let Some(rest) = value.strip_prefix("silo-remote:") else {
        return Ok(Target::Local(value.to_owned()));
    };
    let invalid = || EngineError::internal("Invalid remote computer target.");
    let (device_id, computer_id) = rest.split_once(':').ok_or_else(invalid)?;
    if uuid::Uuid::parse_str(device_id).is_err() || uuid::Uuid::parse_str(computer_id).is_err() {
        return Err(invalid());
    }
    Ok(Target::Remote {
        device_id: device_id.to_owned(),
        computer_id: computer_id.to_owned(),
    })
}

/// The `(computerId, deviceId)` a UI computer argument addresses.
pub(crate) async fn resolve(
    engine: &EngineClient,
    computer: &str,
) -> Result<(String, Option<String>), EngineError> {
    Ok(match parse_target(computer)? {
        Target::Local(name) => (engine.computer_id(&name).await?, None),
        Target::Remote {
            device_id,
            computer_id,
        } => (computer_id, Some(device_id)),
    })
}

/// This device's settings, as the launchers read them.
async fn settings(engine: &EngineClient) -> Result<Map<String, Value>, EngineError> {
    let read = engine.call("settings.read", json!({})).await?;
    Ok(read
        .get("settings")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default())
}

/// Opens `url` in the browser chosen in this device's settings.
pub(crate) async fn open_url(
    engine: &EngineClient,
    url: &str,
    open: impl FnOnce(&Map<String, Value>, &str) -> Result<(), String>,
) -> Result<(), EngineError> {
    let url = crate::applications::validated_browser_url(url).map_err(EngineError::internal)?;
    open(&settings(engine).await?, &url).map_err(EngineError::internal)
}

pub(crate) fn browser(settings: &Map<String, Value>, url: &str) -> Result<(), String> {
    crate::applications::open_browser_with_settings(settings, url)
}

/// Runs `command` (a shell line the engine built) in the terminal chosen in this device's settings.
pub(crate) async fn open_in_terminal(
    engine: &EngineClient,
    command: &str,
    open: impl FnOnce(&Map<String, Value>, &str) -> Result<(), String>,
) -> Result<(), EngineError> {
    open(&settings(engine).await?, command).map_err(EngineError::internal)
}

pub(crate) fn terminal(settings: &Map<String, Value>, command: &str) -> Result<(), String> {
    let application = crate::applications::selected_terminal_from_settings(settings)?;
    crate::applications::open_terminal_with_application(&application, command)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEVICE: &str = "00000000-0000-4000-8000-0000000000d1";
    const COMPUTER: &str = "00000000-0000-4000-8000-0000000000c1";

    fn listing() -> Value {
        json!({
            "linux": {"computers": [{"configuration": {"id": "lin-1", "name": "dev"}}]},
            "macos": {"computers": [{"id": "mac-1", "name": "sonoma"}]}
        })
    }

    #[test]
    fn the_roster_translates_names_and_ids_of_both_kinds() {
        let roster = Roster::from_listing(&listing());
        assert_eq!(roster.id_of("dev").unwrap(), "lin-1");
        assert_eq!(roster.id_of("sonoma").unwrap(), "mac-1");
        assert_eq!(roster.id_of("mac-1").unwrap(), "mac-1");
        assert_eq!(roster.id_of("ghost").unwrap_err().code, "not_found");
        assert_eq!(roster.name_of("lin-1"), "dev");
        assert_eq!(roster.name_of("gone"), "gone");
    }

    #[test]
    fn a_device_is_added_only_when_one_is_named() {
        assert_eq!(with_device(json!({"a": 1}), None), json!({"a": 1}));
        assert_eq!(
            with_device(json!({"a": 1}), Some("d")),
            json!({"a": 1, "deviceId": "d"})
        );
    }

    #[test]
    fn targets_are_local_names_or_well_formed_remote_pairs() {
        assert_eq!(parse_target("dev").unwrap(), Target::Local("dev".into()));
        assert_eq!(
            parse_target(&format!("silo-remote:{DEVICE}:{COMPUTER}")).unwrap(),
            Target::Remote {
                device_id: DEVICE.into(),
                computer_id: COMPUTER.into()
            }
        );
        assert!(parse_target("silo-remote:nope").is_err());
        assert!(parse_target(&format!("silo-remote:{DEVICE}:x")).is_err());
    }
}
