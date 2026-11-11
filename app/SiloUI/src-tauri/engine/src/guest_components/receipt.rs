//! What Silo records about the pieces it keeps current in a computer (`mod.rs`): a receipt in the
//! computer, and a copy on the device that the computer's status reads without asking the
//! computer. The guest's receipt is written by a computer the agent has root in, so it is read
//! as untrusted text: bounded, parsed field by field, and used only to decide what to do, never
//! to believe that something is installed.

use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::COMPONENTS;

/// The receipt's format version.
pub const SCHEMA_VERSION: u32 = 1;
/// A receipt larger than this is not read.
pub const MAX_RECEIPT_BYTES: usize = 64 * 1024;
/// Longest text kept of a step's explanation.
pub const MAX_DETAIL_CHARS: usize = 300;

/// How one piece ended its last reconcile.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    /// In place and current.
    Ok,
    /// The step failed; `detail` says why.
    Failed,
    /// This build of Silo cannot provide the piece.
    Unavailable,
    /// Waiting for something else (an agent that is not installed yet).
    Waiting,
    /// Taken out on purpose (the tools were turned off).
    Removed,
}

/// The result of one piece's last reconcile.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// The component's version when the step ran.
    pub version: u32,
    pub state: State,
    /// The SHA-256 of what was installed, for a piece that is a file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Seconds since the epoch.
    pub at: u64,
}

impl Entry {
    pub fn new(version: u32, state: State, at: u64) -> Self {
        Self {
            version,
            state,
            sha256: None,
            detail: None,
            at,
        }
    }

    pub fn with_detail(mut self, detail: impl AsRef<str>) -> Self {
        self.detail = Some(clip(detail.as_ref()));
        self
    }

    pub fn with_sha256(mut self, sha256: impl Into<String>) -> Self {
        self.sha256 = Some(sha256.into());
        self
    }
}

/// The receipt of one computer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub schema_version: u32,
    pub computer_id: String,
    pub components: BTreeMap<String, Entry>,
}

fn clip(text: &str) -> String {
    let single_line: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    single_line.trim().chars().take(MAX_DETAIL_CHARS).collect()
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn entry_from(value: &Value) -> Option<Entry> {
    let object = value.as_object()?;
    let state: State = serde_json::from_value(object.get("state")?.clone()).ok()?;
    Some(Entry {
        version: u32::try_from(object.get("version")?.as_u64()?).ok()?,
        state,
        sha256: object
            .get("sha256")
            .and_then(Value::as_str)
            .filter(|text| is_sha256(text))
            .map(str::to_owned),
        detail: object
            .get("detail")
            .and_then(Value::as_str)
            .map(clip)
            .filter(|text| !text.is_empty()),
        at: object.get("at").and_then(Value::as_u64).unwrap_or(0),
    })
}

impl Receipt {
    /// A receipt with nothing recorded.
    pub fn empty(computer_id: &str) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            computer_id: computer_id.to_owned(),
            components: BTreeMap::new(),
        }
    }

    /// Reads the receipt `bytes` as the computer `computer_id` holds it. Anything that is not a
    /// receipt of this format for this computer reads as an empty one: text that is too large or
    /// not JSON, another schema version, another computer's id (the receipt came with a fork or an
    /// import), and entries that are malformed or name no component of this build.
    pub fn parse(bytes: &[u8], computer_id: &str) -> Self {
        let empty = Self::empty(computer_id);
        if bytes.len() > MAX_RECEIPT_BYTES {
            return empty;
        }
        let Ok(Value::Object(document)) = serde_json::from_slice::<Value>(bytes) else {
            return empty;
        };
        if document.get("schemaVersion").and_then(Value::as_u64) != Some(u64::from(SCHEMA_VERSION))
            || document.get("computerId").and_then(Value::as_str) != Some(computer_id)
        {
            return empty;
        }
        let mut components = BTreeMap::new();
        if let Some(Value::Object(listed)) = document.get("components") {
            for component in COMPONENTS {
                if let Some(entry) = listed.get(component.id).and_then(entry_from) {
                    components.insert(component.id.to_owned(), entry);
                }
            }
        }
        Self {
            components,
            ..empty
        }
    }

    pub fn to_json(&self) -> String {
        json!(self).to_string()
    }

    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.components.get(id)
    }
}

/// The receipts the device keeps for the status of each computer, and the list of computers whose
/// registration is owed a removal, in `<app data>/guest-client/components/`.
pub struct Reports {
    directory: PathBuf,
}

fn safe_name(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl Reports {
    pub fn new(guest_client_directory: &Path) -> Self {
        Self {
            directory: guest_client_directory.join("components"),
        }
    }

    fn path(&self, id: &str) -> Option<PathBuf> {
        safe_name(id).then(|| self.directory.join(format!("{id}.json")))
    }

    fn owed_path(&self) -> PathBuf {
        self.directory.join("removals.json")
    }

    fn write(&self, path: &Path, text: &str) -> Result<(), String> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.directory)
            .map_err(|error| format!("Silo could not keep the components record: {error}"))?;
        let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
        let written = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary)
            .and_then(|mut file| {
                file.write_all(text.as_bytes())
                    .and_then(|()| file.sync_all())
            })
            .and_then(|()| fs::rename(&temporary, path));
        if written.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        written.map_err(|error| format!("Silo could not keep the components record: {error}"))
    }

    /// The receipt of the last reconcile of `computer_id`, if there was one.
    pub fn read(&self, computer_id: &str) -> Option<Receipt> {
        let path = self.path(computer_id)?;
        let bytes = fs::read(path).ok()?;
        let receipt = Receipt::parse(&bytes, computer_id);
        (!receipt.components.is_empty()).then_some(receipt)
    }

    pub fn save(&self, receipt: &Receipt) -> Result<(), String> {
        let path = self
            .path(&receipt.computer_id)
            .ok_or("That computer has no usable id.")?;
        self.write(&path, &receipt.to_json())
    }

    pub fn forget(&self, computer_id: &str) {
        if let Some(path) = self.path(computer_id) {
            let _ = fs::remove_file(path);
        }
        self.clear_owed(&[computer_id]);
    }

    fn owed_list(&self) -> Vec<String> {
        fs::read(self.owed_path())
            .ok()
            .filter(|bytes| bytes.len() <= MAX_RECEIPT_BYTES)
            .and_then(|bytes| serde_json::from_slice::<Vec<String>>(&bytes).ok())
            .map(|list| list.into_iter().filter(|id| safe_name(id)).collect())
            .unwrap_or_default()
    }

    /// The computers whose registration with the agents is still to be taken out.
    pub fn owed(&self) -> Vec<String> {
        self.owed_list()
    }

    pub fn is_owed(&self, computer_id: &str) -> bool {
        self.owed_list().iter().any(|id| id == computer_id)
    }

    /// Records that these computers' registration must be taken out when they next run.
    pub fn owe_removal(&self, computer_ids: &[&str]) -> Result<(), String> {
        let mut list = self.owed_list();
        for id in computer_ids.iter().filter(|id| safe_name(id)) {
            if !list.iter().any(|known| known == id) {
                list.push((*id).to_owned());
            }
        }
        self.write(&self.owed_path(), &json!(list).to_string())
    }

    pub fn clear_owed(&self, computer_ids: &[&str]) {
        let list = self.owed_list();
        let kept: Vec<&String> = list
            .iter()
            .filter(|id| !computer_ids.contains(&id.as_str()))
            .collect();
        if kept.len() != list.len() {
            let _ = self.write(&self.owed_path(), &json!(kept).to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "b2f8e2dce4c13b320db6c20c8db027d72978297ddddb0956ca5af257185b1881";

    fn sample() -> Receipt {
        let mut receipt = Receipt::empty("c1");
        receipt.components.insert(
            "silo".into(),
            Entry::new(1, State::Ok, 100).with_sha256(SHA),
        );
        receipt.components.insert(
            "silo-mcp".into(),
            Entry::new(1, State::Waiting, 100).with_detail("Codex is not installed yet"),
        );
        receipt
    }

    #[test]
    fn a_receipt_survives_its_own_format() {
        let receipt = sample();
        assert_eq!(Receipt::parse(receipt.to_json().as_bytes(), "c1"), receipt);
        let value: Value = serde_json::from_str(&receipt.to_json()).unwrap();
        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(value["components"]["silo"]["state"], "ok");
        assert_eq!(value["components"]["silo-mcp"]["state"], "waiting");
    }

    #[test]
    fn a_receipt_that_is_too_large_or_not_json_reads_as_empty() {
        let mut big = sample().to_json().into_bytes();
        big.resize(MAX_RECEIPT_BYTES + 1, b' ');
        assert!(Receipt::parse(&big, "c1").components.is_empty());
        for text in [
            &b""[..],
            b"not json",
            b"[1,2,3]",
            b"null",
            b"{\"schemaVersion\":1,\"computerId\":\"c1\",\"components\":",
            &[0xff, 0xfe, 0x00][..],
        ] {
            assert!(Receipt::parse(text, "c1").components.is_empty(), "{text:?}");
        }
        let deep = format!("{}1{}", "[".repeat(5000), "]".repeat(5000));
        assert!(Receipt::parse(deep.as_bytes(), "c1").components.is_empty());
    }

    #[test]
    fn a_receipt_of_another_computer_or_schema_is_not_trusted() {
        let text = sample().to_json();
        assert!(Receipt::parse(text.as_bytes(), "another")
            .components
            .is_empty());
        let other_schema = text.replace("\"schemaVersion\":1", "\"schemaVersion\":2");
        assert!(Receipt::parse(other_schema.as_bytes(), "c1")
            .components
            .is_empty());
        let no_schema =
            r#"{"computerId":"c1","components":{"silo":{"version":1,"state":"ok","at":1}}}"#;
        assert!(Receipt::parse(no_schema.as_bytes(), "c1")
            .components
            .is_empty());
    }

    #[test]
    fn hostile_entries_are_dropped_or_cleaned() {
        let long = "x".repeat(10_000);
        let text = json!({
            "schemaVersion": 1, "computerId": "c1",
            "components": {
                "silo": {"version": 1, "state": "ok", "sha256": "not a hash", "detail": format!("a\nb\u{1b}[31m{long}"), "at": 5},
                "silo-mcp": {"version": "one", "state": "ok", "at": 5},
                "something-else": {"version": 1, "state": "ok", "at": 5},
                "__proto__": {"version": 1, "state": "ok", "at": 5}
            }
        })
        .to_string();
        let receipt = Receipt::parse(text.as_bytes(), "c1");
        assert_eq!(receipt.components.keys().collect::<Vec<_>>(), vec!["silo"]);
        let silo = receipt.entry("silo").unwrap();
        assert_eq!(silo.sha256, None);
        let detail = silo.detail.as_deref().unwrap();
        assert!(detail.chars().count() <= MAX_DETAIL_CHARS);
        assert!(!detail.contains('\n') && !detail.contains('\u{1b}'));
        let wrong_state = json!({"schemaVersion":1,"computerId":"c1","components":{"silo":{"version":1,"state":"installed","at":1}}}).to_string();
        assert!(Receipt::parse(wrong_state.as_bytes(), "c1")
            .components
            .is_empty());
        let huge_version = json!({"schemaVersion":1,"computerId":"c1","components":{"silo":{"version":99999999999u64,"state":"ok","at":1}}}).to_string();
        assert!(Receipt::parse(huge_version.as_bytes(), "c1")
            .components
            .is_empty());
    }

    #[test]
    fn the_device_copy_is_kept_per_computer_and_private() {
        let dir = tempfile::tempdir().unwrap();
        let reports = Reports::new(dir.path());
        assert!(reports.read("c1").is_none());
        reports.save(&sample()).unwrap();
        assert_eq!(reports.read("c1").unwrap(), sample());
        assert!(reports.read("c2").is_none());
        let mode = std::os::unix::fs::PermissionsExt::mode(
            &fs::metadata(dir.path().join("components/c1.json"))
                .unwrap()
                .permissions(),
        ) & 0o777;
        assert_eq!(mode, 0o600);
        reports.forget("c1");
        assert!(reports.read("c1").is_none());
        for hostile in ["", "../x", "a/b", "a b"] {
            assert!(
                reports.save(&Receipt::empty(hostile)).is_err(),
                "{hostile:?}"
            );
        }
    }

    #[test]
    fn removals_are_owed_until_cleared_and_forgotten_with_the_computer() {
        let dir = tempfile::tempdir().unwrap();
        let reports = Reports::new(dir.path());
        assert!(reports.owed().is_empty());
        reports.owe_removal(&["a", "b", "a", "../bad"]).unwrap();
        assert_eq!(reports.owed(), vec!["a", "b"]);
        assert!(reports.is_owed("a") && !reports.is_owed("c"));
        reports.clear_owed(&["a"]);
        assert_eq!(reports.owed(), vec!["b"]);
        reports.forget("b");
        assert!(reports.owed().is_empty());
        fs::write(dir.path().join("components/removals.json"), "{bad").unwrap();
        assert!(reports.owed().is_empty());
    }
}
