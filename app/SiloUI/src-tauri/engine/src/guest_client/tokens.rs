//! Each computer's channel: its loopback port and the SHA-256 of its token, in
//! `<app data>/guest-client/tokens.json`. The token itself is never stored; the engine writes it
//! into the computer when it mints it and keeps only the hash.

use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const PREFIX: &str = "silo_gc_";
/// The longest token the store looks at; a minted one is 72 characters.
pub const MAX_TOKEN_BYTES: usize = 200;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub computer_id: String,
    /// Hex SHA-256 of the computer's token; empty until a token is written into the computer.
    #[serde(default)]
    pub hash: String,
    /// When the tools were turned on, or when an earlier engine minted the token.
    pub minted_at: String,
    /// The channel's port on the device's loopback. A record without one is from the earlier
    /// in-guest client and does not turn the tools on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// The forks this token made, as an earlier engine recorded them here. They are moved to
    /// the policy's record (`policy::store`), which now keeps who made which computer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fork_ids: Vec<String>,
    /// Forks an earlier engine was waiting for.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending: Vec<PendingFork>,
    /// Jobs this token started, as an earlier engine recorded them; moved like `fork_ids`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub jobs: Vec<OwnedJob>,
}

impl Record {
    /// Whether the tools are on: the record has a port.
    pub fn enabled(&self) -> bool {
        self.port.is_some()
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingFork {
    pub name: String,
    #[serde(default)]
    pub job_id: Option<String>,
    pub at: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnedJob {
    pub id: String,
    pub at: u64,
}

/// The most jobs one computer may have started and not acknowledged.
pub const MAX_JOBS: usize = 16;

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// 32 bytes from the operating system's random source.
fn random_hex() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    // SAFETY: the buffer is 32 bytes, the most getentropy accepts.
    if unsafe { libc::getentropy(bytes.as_mut_ptr().cast(), bytes.len()) } != 0 {
        return Err("The system random source is not available.".into());
    }
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub struct Minted {
    pub token: String,
    pub record: Record,
}

/// One file under `<app data>/guest-client/`, owner-only, replaced atomically.
pub struct TokenStore {
    directory: PathBuf,
}

static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Hex SHA-256 of `token`.
pub fn hash_of(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .fold(String::new(), |mut text, byte| {
            use std::fmt::Write as _;
            let _ = write!(text, "{byte:02x}");
            text
        })
}

/// Compares two strings in time that does not depend on where they differ.
pub fn same(left: &str, right: &str) -> bool {
    left.len() == right.len()
        && left
            .bytes()
            .zip(right.bytes())
            .fold(0u8, |difference, (a, b)| difference | (a ^ b))
            == 0
}

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

#[cfg(test)]
thread_local! {
    static TEST_DIRECTORY: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Tests on this thread keep the store in `directory` instead of the app's.
#[cfg(test)]
pub(crate) fn use_test_directory(directory: Option<PathBuf>) {
    TEST_DIRECTORY.with(|test| *test.borrow_mut() = directory);
}

impl TokenStore {
    pub fn new(app_data: &Path) -> Self {
        Self {
            directory: app_data.join("guest-client"),
        }
    }

    /// The running engine's store; `None` before the engine has a context.
    pub fn current() -> Option<Self> {
        #[cfg(test)]
        if let Some(directory) = TEST_DIRECTORY.with(|test| test.borrow().clone()) {
            return Some(Self { directory });
        }
        crate::context::current().map(|context| Self::new(&context.paths().app_data))
    }

    fn file(&self) -> PathBuf {
        self.directory.join("tokens.json")
    }

    fn read(&self) -> Result<Vec<Record>, String> {
        match fs::read_to_string(self.file()) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|error| format!("The tokens of Silo's tools are unreadable: {error}")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(format!("Cannot read the tokens of Silo's tools: {error}")),
        }
    }

    fn write(&self, records: &[Record]) -> Result<(), String> {
        let io = |error: std::io::Error| format!("Cannot save the tokens of Silo's tools: {error}");
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.directory)
            .map_err(io)?;
        let temporary = self
            .directory
            .join(format!("tokens-{}.tmp", uuid::Uuid::new_v4().simple()));
        let text = serde_json::to_vec(records).map_err(|error| error.to_string())?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(io)?;
        file.write_all(&text)
            .and_then(|()| file.sync_all())
            .map_err(io)?;
        fs::rename(&temporary, self.file()).map_err(io)
    }

    /// Turns the tools on for the computer on `port`, or moves them there. A computer that
    /// changes port gets no valid token until a new one is minted, since the old one may have
    /// been sent to whatever held that port.
    pub fn enable(&self, computer_id: &str, port: u16) -> Result<Record, String> {
        if computer_id.is_empty() || port == 0 {
            return Err("Silo's tools need a computer and a port.".into());
        }
        let _held = lock();
        let mut records = self.read()?;
        let record = match records
            .iter_mut()
            .find(|record| record.computer_id == computer_id)
        {
            Some(record) => {
                if record.port != Some(port) {
                    record.port = Some(port);
                    record.hash.clear();
                    record.minted_at = now();
                }
                record.clone()
            }
            None => {
                let record = Record {
                    computer_id: computer_id.to_owned(),
                    hash: String::new(),
                    minted_at: now(),
                    port: Some(port),
                    fork_ids: Vec::new(),
                    pending: Vec::new(),
                    jobs: Vec::new(),
                };
                records.push(record.clone());
                record
            }
        };
        self.write(&records)?;
        Ok(record)
    }

    /// Mints a fresh token for a computer whose tools are on, replacing the one it had: the
    /// old token stops working at once.
    pub fn mint(&self, computer_id: &str) -> Result<Minted, String> {
        let _held = lock();
        let token = format!("{PREFIX}{}", random_hex()?);
        let mut records = self.read()?;
        let record = records
            .iter_mut()
            .find(|record| record.computer_id == computer_id && record.enabled())
            .ok_or_else(|| "Silo's tools are not on in that computer.".to_owned())?;
        record.hash = hash_of(&token);
        let record = record.clone();
        self.write(&records)?;
        Ok(Minted { token, record })
    }

    /// The record a token belongs to, only when it was minted for `computer_id` and that
    /// computer's tools are on: a token is valid on its own computer's channel alone.
    pub fn verify(&self, token: &str, computer_id: &str) -> Option<Record> {
        if !token.starts_with(PREFIX) || token.len() > MAX_TOKEN_BYTES {
            return None;
        }
        let hash = hash_of(token);
        let _held = lock();
        self.read().ok()?.into_iter().find(|record| {
            record.computer_id == computer_id
                && record.enabled()
                && !record.hash.is_empty()
                && same(&record.hash, &hash)
        })
    }

    /// Changes this computer's record under the store's lock and saves it when `change`
    /// succeeds. `Ok(None)` when the computer has no record.
    pub fn update<R>(
        &self,
        computer_id: &str,
        change: impl FnOnce(&mut Record) -> Result<R, String>,
    ) -> Result<Option<R>, String> {
        let _held = lock();
        let mut records = self.read()?;
        let Some(record) = records
            .iter_mut()
            .find(|record| record.computer_id == computer_id)
        else {
            return Ok(None);
        };
        let result = change(record)?;
        self.write(&records)?;
        Ok(Some(result))
    }

    /// The computer's record when its tools are on.
    pub fn status(&self, computer_id: &str) -> Result<Option<Record>, String> {
        let _held = lock();
        Ok(self
            .read()?
            .into_iter()
            .find(|record| record.computer_id == computer_id && record.enabled()))
    }

    /// The computer's channel port when its tools are on.
    pub fn port_of(&self, computer_id: &str) -> Option<u16> {
        self.status(computer_id).ok().flatten()?.port
    }

    /// The records of computers whose tools are on.
    pub fn enabled(&self) -> Vec<Record> {
        let _held = lock();
        self.read()
            .unwrap_or_default()
            .into_iter()
            .filter(Record::enabled)
            .collect()
    }

    /// Every record, including those an earlier engine left without a port.
    pub fn all(&self) -> Result<Vec<Record>, String> {
        let _held = lock();
        self.read()
    }

    /// Removes the records of these computers; returns how many were removed.
    pub fn revoke(&self, computer_ids: &[&str]) -> Result<usize, String> {
        let _held = lock();
        let mut records = self.read()?;
        let before = records.len();
        records.retain(|record| !computer_ids.contains(&record.computer_id.as_str()));
        let removed = before - records.len();
        if removed > 0 {
            self.write(&records)?;
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_32_random_bytes_and_needs_the_tools_on() {
        let dir = tempfile::tempdir().unwrap();
        let store = TokenStore::new(dir.path());
        assert!(store.mint("main").is_err());
        store.enable("main", 40001).unwrap();
        let minted = store.mint("main").unwrap();
        assert_eq!(minted.token.len(), PREFIX.len() + 64);
        assert_eq!(minted.record.port, Some(40001));
        assert!(store.update("other", |_| Ok(())).unwrap().is_none());
    }

    #[test]
    fn only_the_hash_is_stored_and_the_token_verifies_for_its_computer_alone() {
        let dir = tempfile::tempdir().unwrap();
        let store = TokenStore::new(dir.path());
        store.enable("main", 40001).unwrap();
        store.enable("fork-1", 40002).unwrap();
        let minted = store.mint("main").unwrap();
        let text = fs::read_to_string(store.file()).unwrap();
        assert!(!text.contains(&minted.token));
        assert!(text.contains(&minted.record.hash));
        assert!(store.verify(&minted.token, "main").is_some());
        assert!(store.verify(&minted.token, "fork-1").is_none());
        assert!(store.verify("silo_gc_nope", "main").is_none());
        assert!(store
            .verify(&minted.token[PREFIX.len()..], "main")
            .is_none());
        let mode = fs::metadata(store.file()).unwrap();
        assert_eq!(
            std::os::unix::fs::PermissionsExt::mode(&mode.permissions()) & 0o777,
            0o600
        );
    }

    #[test]
    fn a_computer_with_no_token_written_yet_accepts_none() {
        let dir = tempfile::tempdir().unwrap();
        let store = TokenStore::new(dir.path());
        store.enable("main", 40001).unwrap();
        // The hash of an empty record never matches any token.
        assert!(store
            .verify(&format!("{PREFIX}{}", "0".repeat(64)), "main")
            .is_none());
        assert!(store.status("main").unwrap().unwrap().hash.is_empty());
    }

    #[test]
    fn minting_again_replaces_the_token_and_revoking_removes_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = TokenStore::new(dir.path());
        store.enable("main", 40001).unwrap();
        let first = store.mint("main").unwrap();
        let second = store.mint("main").unwrap();
        assert_ne!(first.token, second.token);
        assert!(store.verify(&first.token, "main").is_none());
        assert!(store.verify(&second.token, "main").is_some());
        assert_eq!(store.enabled().len(), 1);
        assert_eq!(store.revoke(&["other"]).unwrap(), 0);
        assert_eq!(store.revoke(&["main"]).unwrap(), 1);
        assert!(store.verify(&second.token, "main").is_none());
        assert!(store.status("main").unwrap().is_none());
    }

    #[test]
    fn moving_to_another_port_ends_the_token_and_the_same_port_keeps_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = TokenStore::new(dir.path());
        store.enable("main", 40001).unwrap();
        let minted = store.mint("main").unwrap();
        store.enable("main", 40001).unwrap();
        assert!(store.verify(&minted.token, "main").is_some());
        store.enable("main", 40009).unwrap();
        assert!(store.verify(&minted.token, "main").is_none());
        assert_eq!(store.port_of("main"), Some(40009));
    }

    #[test]
    fn a_record_of_the_earlier_client_loads_and_does_not_turn_the_tools_on() {
        let dir = tempfile::tempdir().unwrap();
        let store = TokenStore::new(dir.path());
        fs::create_dir_all(&store.directory).unwrap();
        fs::write(
            store.file(),
            r#"[{"computerId":"old","hash":"ab","mintedAt":"x","policy":{"forkSecrets":[]},"forkIds":["f"],"jobs":[]}]"#,
        )
        .unwrap();
        assert!(store.enabled().is_empty());
        assert!(store.status("old").unwrap().is_none());
        assert_eq!(store.all().unwrap()[0].fork_ids, ["f"]);
        assert!(store.mint("old").is_err());
        assert!(store.enable("main", 0).is_err());
    }
}
