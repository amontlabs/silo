//! Each computer's channel: its loopback port and the SHA-256 of its token, in
//! `<app data>/guest-client/tokens.json`. The token itself is never stored; the engine writes it
//! into the computer when it mints it and keeps only the hash, which is the key the computer's
//! handshake proof is checked with (`silo_api::transport::channel_proof`). Ports a computer's
//! network may still name after its tools were turned off or moved are kept in
//! `<app data>/guest-client/held-ports.json`.

use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};


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
    /// The channel moved to a new port while the computer's network may still name an old one
    /// that another program holds. Its channel is not served and no token is written until the
    /// computer runs with rules that match.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub restart_needed: bool,
}

/// A port a computer's network may still name after its tools were turned off or its channel
/// moved. The engine keeps it bound, accepting and closing every connection, until the computer
/// runs with rules that no longer name it, or is deleted.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeldPort {
    pub computer_id: String,
    pub port: u16,
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
pub fn random_bytes() -> Result<[u8; 32], String> {
    let mut bytes = [0u8; 32];
    // SAFETY: the buffer is 32 bytes, the most getentropy accepts.
    if unsafe { libc::getentropy(bytes.as_mut_ptr().cast(), bytes.len()) } != 0 {
        return Err("The system random source is not available.".into());
    }
    Ok(bytes)
}

fn random_hex() -> Result<String, String> {
    random_bytes().map(|bytes| silo_api::transport::to_hex(&bytes))
}

pub struct Minted {
    pub token: String,
    pub record: Record,
}

/// Two files under `<app data>/guest-client/`, owner-only, each replaced atomically.
pub struct TokenStore {
    directory: PathBuf,
}

/// The store, held under its lock: what is read and saved through it cannot change in between.
pub struct Locked<'a> {
    store: &'a TokenStore,
    _held: std::sync::MutexGuard<'static, ()>,
}

impl Locked<'_> {
    /// Every record, including those an earlier engine left without a port.
    pub fn records(&self) -> Result<Vec<Record>, String> {
        self.store.read()
    }

    pub fn save(&self, records: &[Record]) -> Result<(), String> {
        self.store.write_file("tokens", records)
    }

    /// The ports kept for computers' networks that may still name them.
    pub fn held(&self) -> Result<Vec<HeldPort>, String> {
        self.store.read_held()
    }

    pub fn save_held(&self, held: &[HeldPort]) -> Result<(), String> {
        self.store.write_file("held-ports", held)
    }
}

/// Puts `computer_id`'s channel on `port` in `records`, turning its tools on when it had no
/// record. A computer that changes port gets no valid token until a new one is minted, since the
/// old one may have been sent to whatever held that port, and the port it leaves is held for
/// it. Returns the computer's record.
pub fn set_port(
    records: &mut Vec<Record>,
    held: &mut Vec<HeldPort>,
    computer_id: &str,
    port: u16,
) -> Result<Record, String> {
    if computer_id.is_empty() || port == 0 {
        return Err("Silo's tools need a computer and a port.".into());
    }
    held.retain(|entry| !(entry.computer_id == computer_id && entry.port == port));
    let record = match records
        .iter_mut()
        .find(|record| record.computer_id == computer_id)
    {
        Some(record) => {
            if record.port != Some(port) {
                if let Some(old) = record.port {
                    hold(held, computer_id, old);
                }
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
                restart_needed: false,
            };
            records.push(record.clone());
            record
        }
    };
    Ok(record)
}

/// Adds `port` to the ports held for `computer_id`, once.
pub fn hold(held: &mut Vec<HeldPort>, computer_id: &str, port: u16) {
    if !held
        .iter()
        .any(|entry| entry.computer_id == computer_id && entry.port == port)
    {
        held.push(HeldPort {
            computer_id: computer_id.to_owned(),
            port,
        });
    }
}

static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Hex SHA-256 of `token`: the key its channel proofs are made with.
pub fn hash_of(token: &str) -> String {
    silo_api::transport::to_hex(&silo_api::transport::channel_key(token))
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

    fn held_file(&self) -> PathBuf {
        self.directory.join("held-ports.json")
    }

    fn read_held(&self) -> Result<Vec<HeldPort>, String> {
        match fs::read_to_string(self.held_file()) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|error| format!("The ports of Silo's tools are unreadable: {error}")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(format!("Cannot read the ports of Silo's tools: {error}")),
        }
    }

    fn write(&self, records: &[Record]) -> Result<(), String> {
        self.write_file("tokens", records)
    }

    fn write_file<T: Serialize + ?Sized>(&self, name: &str, value: &T) -> Result<(), String> {
        let io = |error: std::io::Error| format!("Cannot save the tokens of Silo's tools: {error}");
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.directory)
            .map_err(io)?;
        let temporary = self
            .directory
            .join(format!("{name}-{}.tmp", uuid::Uuid::new_v4().simple()));
        let text = serde_json::to_vec(value).map_err(|error| error.to_string())?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(io)?;
        file.write_all(&text)
            .and_then(|()| file.sync_all())
            .map_err(io)?;
        fs::rename(&temporary, self.directory.join(format!("{name}.json"))).map_err(io)
    }

    /// The store under its lock, for a change that must see and save both files at once.
    pub fn locked(&self) -> Locked<'_> {
        Locked {
            store: self,
            _held: lock(),
        }
    }

    /// Turns the tools on for the computer on `port`, or moves them there (see [`set_port`]).
    pub fn enable(&self, computer_id: &str, port: u16) -> Result<Record, String> {
        let locked = self.locked();
        let mut records = locked.records()?;
        let mut held = locked.held()?;
        let before = held.clone();
        let record = set_port(&mut records, &mut held, computer_id, port)?;
        if held != before {
            locked.save_held(&held)?;
        }
        locked.save(&records)?;
        Ok(record)
    }

    /// Mints a fresh token for a computer whose tools are on, replacing the one it had: the
    /// old token stops working at once, and every session opened with it ends.
    pub fn mint(&self, computer_id: &str) -> Result<Minted, String> {
        let minted = {
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
            Minted { token, record }
        };
        if let Some(channels) = super::channels() {
            channels.end_sessions(computer_id);
        }
        Ok(minted)
    }

    /// The record whose token made `proof` for `nonce` on `port`, only when that is
    /// `computer_id`'s record, its tools are on, its channel is on `port` and is not waiting for
    /// a restart: a token is valid on its own computer's port alone.
    pub fn verify(&self, computer_id: &str, port: u16, nonce: &[u8], proof: &str) -> Option<Record> {
        let proof = silo_api::transport::from_hex::<32>(proof)?;
        let _held = lock();
        self.read().ok()?.into_iter().find(|record| {
            record.computer_id == computer_id
                && record.port == Some(port)
                && !record.restart_needed
                && silo_api::transport::from_hex::<32>(&record.hash).is_some_and(|key| {
                    silo_api::transport::channel_proof_matches(&key, nonce, port, &proof)
                })
        })
    }

    /// Forgets every token hash: tokens minted before now stop working, and each running
    /// computer gets a new one when its endpoint file is next checked.
    pub fn clear_hashes(&self) -> Result<(), String> {
        let _held = lock();
        let mut records = self.read()?;
        if records.iter().all(|record| record.hash.is_empty()) {
            return Ok(());
        }
        for record in &mut records {
            record.hash.clear();
        }
        self.write(&records)
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
pub(crate) mod tests {
    use super::*;

    /// Whether `store` serves a handshake proved with `token` on `port` as `computer`.
    pub(crate) fn accepts(store: &TokenStore, token: &str, computer: &str, port: u16) -> bool {
        let nonce = random_bytes().unwrap();
        let proof = silo_api::transport::channel_proof(
            &silo_api::transport::channel_key(token),
            &nonce,
            port,
        );
        store
            .verify(computer, port, &nonce, &silo_api::transport::to_hex(&proof))
            .is_some()
    }

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
    fn only_the_hash_is_stored_and_a_proof_verifies_for_its_computer_alone() {
        let dir = tempfile::tempdir().unwrap();
        let store = TokenStore::new(dir.path());
        store.enable("main", 40001).unwrap();
        store.enable("fork-1", 40002).unwrap();
        let minted = store.mint("main").unwrap();
        let text = fs::read_to_string(store.file()).unwrap();
        assert!(!text.contains(&minted.token));
        assert!(text.contains(&minted.record.hash));
        assert!(accepts(&store, &minted.token, "main", 40001));
        assert!(!accepts(&store, &minted.token, "fork-1", 40002));
        assert!(!accepts(&store, "silo_gc_nope", "main", 40001));
        assert!(!accepts(&store, &minted.token[PREFIX.len()..], "main", 40001));
        let mode = fs::metadata(store.file()).unwrap();
        assert_eq!(
            std::os::unix::fs::PermissionsExt::mode(&mode.permissions()) & 0o777,
            0o600
        );
    }

    #[test]
    fn a_proof_is_good_only_on_the_records_port_for_its_nonce() {
        let dir = tempfile::tempdir().unwrap();
        let store = TokenStore::new(dir.path());
        store.enable("main", 40001).unwrap();
        let minted = store.mint("main").unwrap();
        // Made for another port: refused on that port (not the record's) and on this one.
        assert!(!accepts(&store, &minted.token, "main", 40009));
        let key = silo_api::transport::channel_key(&minted.token);
        let nonce = [3_u8; 32];
        let elsewhere = silo_api::transport::to_hex(&silo_api::transport::channel_proof(
            &key, &nonce, 40009,
        ));
        assert!(store.verify("main", 40001, &nonce, &elsewhere).is_none());
        let here =
            silo_api::transport::to_hex(&silo_api::transport::channel_proof(&key, &nonce, 40001));
        assert!(store.verify("main", 40001, &nonce, &here).is_some());
        assert!(store.verify("main", 40001, &[4_u8; 32], &here).is_none());
        assert!(store.verify("main", 40001, &nonce, &here[..62]).is_none());
        // Waiting for a restart, nothing is served.
        store
            .update("main", |record| {
                record.restart_needed = true;
                Ok(())
            })
            .unwrap();
        assert!(store.verify("main", 40001, &nonce, &here).is_none());
    }

    #[test]
    fn a_computer_with_no_token_written_yet_accepts_none() {
        let dir = tempfile::tempdir().unwrap();
        let store = TokenStore::new(dir.path());
        store.enable("main", 40001).unwrap();
        // The hash of an empty record never matches any token.
        assert!(!accepts(&store, &format!("{PREFIX}{}", "0".repeat(64)), "main", 40001));
        assert!(!accepts(&store, "", "main", 40001));
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
        assert!(!accepts(&store, &first.token, "main", 40001));
        assert!(accepts(&store, &second.token, "main", 40001));
        assert_eq!(store.enabled().len(), 1);
        assert_eq!(store.revoke(&["other"]).unwrap(), 0);
        assert_eq!(store.revoke(&["main"]).unwrap(), 1);
        assert!(!accepts(&store, &second.token, "main", 40001));
        assert!(store.status("main").unwrap().is_none());
    }

    #[test]
    fn clearing_the_hashes_ends_every_token() {
        let dir = tempfile::tempdir().unwrap();
        let store = TokenStore::new(dir.path());
        store.enable("main", 40001).unwrap();
        store.enable("fork", 40002).unwrap();
        let main = store.mint("main").unwrap().token;
        let fork = store.mint("fork").unwrap().token;
        store.clear_hashes().unwrap();
        assert!(!accepts(&store, &main, "main", 40001));
        assert!(!accepts(&store, &fork, "fork", 40002));
        assert_eq!(store.port_of("main"), Some(40001), "the tools stay on");
    }

    #[test]
    fn moving_to_another_port_ends_the_token_holds_the_old_port_and_the_same_port_keeps_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = TokenStore::new(dir.path());
        store.enable("main", 40001).unwrap();
        let minted = store.mint("main").unwrap();
        store.enable("main", 40001).unwrap();
        assert!(accepts(&store, &minted.token, "main", 40001));
        assert!(store.locked().held().unwrap().is_empty());
        store.enable("main", 40009).unwrap();
        assert!(!accepts(&store, &minted.token, "main", 40009));
        assert_eq!(store.port_of("main"), Some(40009));
        assert_eq!(
            store.locked().held().unwrap(),
            vec![HeldPort {
                computer_id: "main".into(),
                port: 40001
            }]
        );
        // Moving back takes the port out of the held ones.
        store.enable("main", 40001).unwrap();
        assert_eq!(store.locked().held().unwrap().len(), 1);
        assert_eq!(store.locked().held().unwrap()[0].port, 40009);
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
        assert!(!store.all().unwrap()[0].restart_needed);
        assert!(store.mint("old").is_err());
        assert!(store.enable("main", 0).is_err());
    }
}
