//! Per-computer tokens for the in-guest client. Only the SHA-256 of a token is stored; the
//! token itself is shown once, when it is minted, and is installed in the guest by the caller.

use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use silo_api::methods::guest_client::GuestClientPolicy;

const PREFIX: &str = "silo_gc_";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub computer_id: String,
    /// Hex SHA-256 of the token.
    pub hash: String,
    pub minted_at: String,
    pub policy: GuestClientPolicy,
    /// Names of the forks this token asked for. Only these are the agent's to start, stop,
    /// diff or ask review for.
    #[serde(default)]
    pub fork_names: Vec<String>,
}

/// The most forks one token may ask for.
pub const MAX_FORKS: usize = 32;

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

fn hash_of(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .fold(String::new(), |mut text, byte| {
            use std::fmt::Write as _;
            let _ = write!(text, "{byte:02x}");
            text
        })
}

fn same(left: &str, right: &str) -> bool {
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

impl TokenStore {
    pub fn new(app_data: &Path) -> Self {
        Self {
            directory: app_data.join("guest-client"),
        }
    }

    fn file(&self) -> PathBuf {
        self.directory.join("tokens.json")
    }

    fn read(&self) -> Result<Vec<Record>, String> {
        match fs::read_to_string(self.file()) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|error| format!("The guest client tokens are unreadable: {error}")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(format!("Cannot read the guest client tokens: {error}")),
        }
    }

    fn write(&self, records: &[Record]) -> Result<(), String> {
        let io = |error: std::io::Error| format!("Cannot save the guest client tokens: {error}");
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

    /// Mints a fresh token for the computer, replacing the one it had: the old token stops
    /// working at once.
    pub fn mint(&self, computer_id: &str, policy: GuestClientPolicy) -> Result<Minted, String> {
        if computer_id.is_empty() {
            return Err("A guest client token needs a computer.".into());
        }
        let _held = lock();
        let token = format!("{PREFIX}{}", random_hex()?);
        let record = Record {
            computer_id: computer_id.to_owned(),
            hash: hash_of(&token),
            minted_at: now(),
            policy,
            fork_names: Vec::new(),
        };
        let mut records = self.read()?;
        records.retain(|existing| existing.computer_id != computer_id);
        records.push(record.clone());
        self.write(&records)?;
        Ok(Minted { token, record })
    }

    /// The record a token belongs to, only when it was minted for `computer_id`: a token found
    /// in another computer (a fork that kept the file) is not valid there.
    pub fn verify(&self, token: &str, computer_id: &str) -> Option<Record> {
        if !token.starts_with(PREFIX) || token.len() > 200 {
            return None;
        }
        let hash = hash_of(token);
        let _held = lock();
        self.read()
            .ok()?
            .into_iter()
            .find(|record| record.computer_id == computer_id && same(&record.hash, &hash))
    }

    /// Records that this computer's token asked for a fork named `name`.
    pub fn note_fork(&self, computer_id: &str, name: &str) -> Result<(), String> {
        let _held = lock();
        let mut records = self.read()?;
        let record = records
            .iter_mut()
            .find(|record| record.computer_id == computer_id)
            .ok_or("This computer has no token.")?;
        if record.fork_names.iter().any(|known| known == name) {
            return Ok(());
        }
        if record.fork_names.len() >= MAX_FORKS {
            return Err(format!(
                "This token has asked for {MAX_FORKS} forks already."
            ));
        }
        if !record.fork_names.iter().any(|known| known == name) {
            record.fork_names.push(name.to_owned());
        }
        self.write(&records)
    }

    pub fn status(&self, computer_id: &str) -> Result<Option<Record>, String> {
        let _held = lock();
        Ok(self
            .read()?
            .into_iter()
            .find(|record| record.computer_id == computer_id))
    }

    pub fn enabled(&self) -> Vec<Record> {
        let _held = lock();
        self.read().unwrap_or_default()
    }

    /// Removes the tokens of these computers; returns how many were removed.
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

    fn policy() -> GuestClientPolicy {
        GuestClientPolicy {
            fork_secrets: vec![],
            agent: None,
        }
    }

    #[test]
    fn a_token_is_32_random_bytes_and_remembers_the_forks_it_asked_for_up_to_a_cap() {
        let dir = tempfile::tempdir().unwrap();
        let store = TokenStore::new(dir.path());
        let minted = store.mint("main", policy()).unwrap();
        assert_eq!(minted.token.len(), PREFIX.len() + 64);
        assert!(store.note_fork("other", "x").is_err());
        for number in 0..MAX_FORKS {
            store.note_fork("main", &format!("f{number}")).unwrap();
        }
        store.note_fork("main", "f0").unwrap();
        assert!(store.note_fork("main", "one-more").is_err());
        assert_eq!(
            store.status("main").unwrap().unwrap().fork_names.len(),
            MAX_FORKS
        );
        assert!(store
            .mint("main", policy())
            .unwrap()
            .record
            .fork_names
            .is_empty());
    }

    #[test]
    fn only_the_hash_is_stored_and_the_token_verifies_for_its_computer_alone() {
        let dir = tempfile::tempdir().unwrap();
        let store = TokenStore::new(dir.path());
        let minted = store.mint("main", policy()).unwrap();
        let text = fs::read_to_string(store.file()).unwrap();
        assert!(!text.contains(&minted.token));
        assert!(text.contains(&minted.record.hash));
        assert!(store.verify(&minted.token, "main").is_some());
        assert!(store.verify(&minted.token, "fork-1").is_none());
        assert!(store.verify("silo_gc_nope", "main").is_none());
        let mode = fs::metadata(store.file()).unwrap();
        assert_eq!(
            std::os::unix::fs::PermissionsExt::mode(&mode.permissions()) & 0o777,
            0o600
        );
    }

    #[test]
    fn minting_again_replaces_the_token_and_revoking_removes_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = TokenStore::new(dir.path());
        let first = store.mint("main", policy()).unwrap();
        let second = store.mint("main", policy()).unwrap();
        assert_ne!(first.token, second.token);
        assert!(store.verify(&first.token, "main").is_none());
        assert!(store.verify(&second.token, "main").is_some());
        assert_eq!(store.enabled().len(), 1);
        assert_eq!(store.revoke(&["other"]).unwrap(), 0);
        assert_eq!(store.revoke(&["main"]).unwrap(), 1);
        assert!(store.verify(&second.token, "main").is_none());
        assert!(store.status("main").unwrap().is_none());
    }
}
