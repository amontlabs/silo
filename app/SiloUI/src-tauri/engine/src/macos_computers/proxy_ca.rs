//! Per-computer proxy CA storage. The private key stays in the channel Keychain.
//!
//! Certificate generation is injected: the current engine dependency graph has no
//! supported X.509 CA generator. The caller must supply matching, unencrypted PEM
//! certificate and private-key bytes from a qualified local generator.

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock, PoisonError, Weak},
};
use zeroize::{Zeroize, Zeroizing};

use crate::channel::{Channel, Keychain};

const ACCOUNT_PREFIX: &str = "computer:";
const MATERIAL_VERSION: u32 = 1;
const MAX_PEM_BYTES: usize = 64 * 1024;
const MAX_STORED_BYTES: usize = MAX_PEM_BYTES * 2 + 4096;

/// A Keychain storage seam used by production and deterministic unit tests.
pub(super) trait CaStore: Send + Sync {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, String>;
    fn set(&self, service: &str, account: &str, value: &str) -> Result<(), String>;
    /// Removes the item; an item that is already absent is not an error.
    fn delete(&self, service: &str, account: &str) -> Result<(), String>;
}

/// PEM material intentionally has no `Debug`, `Serialize`, or `Clone` implementation.
pub(super) struct CaMaterial {
    certificate_pem: String,
    private_key_pem: String,
}

impl CaMaterial {
    /// Construct material from a local generator or a previously decoded Keychain item.
    ///
    /// This checks PEM framing and size, not that the certificate and key form a pair.
    /// The generator must establish that cryptographic relationship.
    pub(super) fn from_pem(
        certificate_pem: String,
        mut private_key_pem: String,
    ) -> Result<Self, String> {
        if !valid_material(&certificate_pem, &private_key_pem) {
            private_key_pem.zeroize();
            return Err("The macOS proxy CA PEM material is invalid.".into());
        }
        Ok(Self {
            certificate_pem,
            private_key_pem,
        })
    }

    pub(super) fn certificate_pem(&self) -> &str {
        &self.certificate_pem
    }

    pub(super) fn private_key_pem(&self) -> &str {
        &self.private_key_pem
    }

    pub(super) fn certificate_fingerprint(&self) -> Result<String, String> {
        let der = pem_der(&self.certificate_pem)?;
        Ok(Sha256::digest(der)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }
}

impl Drop for CaMaterial {
    fn drop(&mut self) {
        self.private_key_pem.zeroize();
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredMaterial {
    version: u32,
    certificate_pem: String,
    private_key_pem: String,
}

impl Drop for StoredMaterial {
    fn drop(&mut self) {
        self.private_key_pem.zeroize();
    }
}

struct KeychainStore;

impl CaStore for KeychainStore {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, String> {
        let entry = keyring::Entry::new(service, account)
            .map_err(|_| "Cannot access the macOS proxy CA in the system credential store.")?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(crate::credential_store::describe(
                &error,
                "Cannot read the macOS proxy CA from the system credential store.",
            )),
        }
    }

    fn set(&self, service: &str, account: &str, value: &str) -> Result<(), String> {
        keyring::Entry::new(service, account)
            .and_then(|entry| entry.set_password(value))
            .map_err(|error| {
                crate::credential_store::describe(
                    &error,
                    "Cannot save the macOS proxy CA in the system credential store.",
                )
            })
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(service, account)
            .map_err(|_| "Cannot access the macOS proxy CA in the system credential store.")?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(crate::credential_store::describe(
                &error,
                "Cannot remove the macOS proxy CA from the system credential store.",
            )),
        }
    }
}

/// The store ordinary calls use. Unit tests never reach the live Keychain.
#[cfg(not(test))]
fn live_store() -> &'static dyn CaStore {
    &KeychainStore
}

#[cfg(test)]
fn live_store() -> &'static dyn CaStore {
    static STORE: OnceLock<tests::MemoryStore> = OnceLock::new();
    STORE.get_or_init(tests::MemoryStore::default)
}

/// Locks are shared only while some caller holds one, so the map does not grow with the
/// number of computers ever created.
type ItemLocks = HashMap<(String, String), Weak<Mutex<()>>>;
static ITEM_LOCKS: OnceLock<Mutex<ItemLocks>> = OnceLock::new();

fn item_locks() -> std::sync::MutexGuard<'static, ItemLocks> {
    ITEM_LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

fn item_lock(service: &str, account: &str) -> Arc<Mutex<()>> {
    let mut locks = item_locks();
    locks.retain(|_, lock| lock.strong_count() != 0);
    let key = (service.to_owned(), account.to_owned());
    if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(Mutex::new(()));
    locks.insert(key, Arc::downgrade(&lock));
    lock
}

/// Drops the lock entries nobody holds any more.
fn prune_item_locks() {
    item_locks().retain(|_, lock| lock.strong_count() != 0);
}

/// Get the stable CA for the current Silo build channel, creating it once if absent.
///
/// `generate` runs only for an absent item. It must return a matching CA certificate
/// and an unencrypted private key as PEM. No PEM is written to application files.
#[allow(dead_code)] // Provisioning owns the explicit CA creation step.
pub(super) fn get_or_create(
    computer_id: &str,
    generate: impl FnOnce() -> Result<CaMaterial, String>,
) -> Result<CaMaterial, String> {
    get_or_create_with(
        crate::channel::current(),
        computer_id,
        live_store(),
        generate,
    )
}

/// Read the channel-scoped CA without creating or replacing Keychain state.
/// Ordinary Start uses this to fail closed when setup did not persist the CA.
pub(super) fn get_existing(computer_id: &str) -> Result<Option<CaMaterial>, String> {
    get_existing_with(crate::channel::current(), computer_id, live_store())
}

/// Removes the computer's CA, key included, from the channel Keychain, and releases the
/// computer's lock entry. Call it when the computer is deleted; it can be repeated until it
/// succeeds, and an absent CA is not an error.
pub(super) fn retire(computer_id: &str) -> Result<(), String> {
    retire_with(crate::channel::current(), computer_id, live_store())
}

pub(super) fn retire_with(
    channel: Channel,
    computer_id: &str,
    store: &dyn CaStore,
) -> Result<(), String> {
    validate_computer_id(computer_id)?;
    let service = service_name(channel);
    let account = account_name(computer_id);
    let result = {
        let item_lock = item_lock(&service, &account);
        let _item_guard = item_lock.lock().unwrap_or_else(PoisonError::into_inner);
        store.delete(&service, &account).and_then(|()| {
            // A deletion that reported success but left the item is not retired.
            match store.get(&service, &account)? {
                None => Ok(()),
                Some(mut left) => {
                    left.zeroize();
                    Err("The macOS proxy CA could not be removed from the credential store.".into())
                }
            }
        })
    };
    prune_item_locks();
    result
}

fn get_existing_with(
    channel: Channel,
    computer_id: &str,
    store: &dyn CaStore,
) -> Result<Option<CaMaterial>, String> {
    validate_computer_id(computer_id)?;
    let service = service_name(channel);
    let account = account_name(computer_id);
    let item_lock = item_lock(&service, &account);
    let _item_guard = item_lock.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(mut value) = store.get(&service, &account)? else {
        return Ok(None);
    };
    let result = decode(&value);
    value.zeroize();
    result.map(Some)
}

/// Get-or-create using an injected store. This keeps tests away from the live Keychain.
pub(super) fn get_or_create_with(
    channel: Channel,
    computer_id: &str,
    store: &dyn CaStore,
    generate: impl FnOnce() -> Result<CaMaterial, String>,
) -> Result<CaMaterial, String> {
    validate_computer_id(computer_id)?;
    let service = service_name(channel);
    let account = account_name(computer_id);
    let item_lock = item_lock(&service, &account);
    let _item_guard = item_lock.lock().unwrap_or_else(PoisonError::into_inner);

    if let Some(mut value) = store.get(&service, &account)? {
        let result = decode(&value);
        value.zeroize();
        return result;
    }

    let material = generate().map_err(|_| "Could not generate the macOS proxy CA.")?;
    let value = encode(&material)?;
    store.set(&service, &account, &value)?;

    // A successful Keychain write is not treated as durable until it can be read back.
    match store.get(&service, &account)? {
        Some(mut stored) => {
            let verified = stored == value.as_str();
            let result = if verified {
                decode(&stored)
            } else {
                Err("The macOS proxy CA could not be verified in the credential store.".into())
            };
            stored.zeroize();
            result
        }
        None => Err("The macOS proxy CA could not be verified in the credential store.".into()),
    }
}

fn validate_computer_id(computer_id: &str) -> Result<(), String> {
    if computer_id.is_empty()
        || computer_id.len() > 64
        || !computer_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err("Invalid macOS computer identifier.".into());
    }
    Ok(())
}

fn service_name(channel: Channel) -> String {
    channel.keychain_service(Keychain::ProxyCa).to_owned()
}

fn account_name(computer_id: &str) -> String {
    format!("{ACCOUNT_PREFIX}{computer_id}")
}

fn encode(material: &CaMaterial) -> Result<Zeroizing<String>, String> {
    if !valid_material(&material.certificate_pem, &material.private_key_pem) {
        return Err("The macOS proxy CA PEM material is invalid.".into());
    }
    let stored = StoredMaterial {
        version: MATERIAL_VERSION,
        certificate_pem: material.certificate_pem.clone(),
        private_key_pem: material.private_key_pem.clone(),
    };
    let encoded =
        Zeroizing::new(serde_json::to_string(&stored).map_err(|_| {
            "Cannot encode the macOS proxy CA for the credential store.".to_owned()
        })?);
    if encoded.len() > MAX_STORED_BYTES {
        return Err("The macOS proxy CA exceeds the supported size.".into());
    }
    Ok(encoded)
}

fn decode(encoded: &str) -> Result<CaMaterial, String> {
    if encoded.len() > MAX_STORED_BYTES {
        return Err("The stored macOS proxy CA exceeds the supported size.".into());
    }
    let mut stored: StoredMaterial = serde_json::from_str(encoded)
        .map_err(|_| "The stored macOS proxy CA is invalid.".to_owned())?;
    if stored.version != MATERIAL_VERSION {
        return Err("The stored macOS proxy CA version is unsupported.".into());
    }
    let cert = std::mem::take(&mut stored.certificate_pem);
    let key = std::mem::take(&mut stored.private_key_pem);
    CaMaterial::from_pem(cert, key).map_err(|_| "The stored macOS proxy CA is invalid.".into())
}

fn valid_pem(pem: &str, allowed_labels: &[&str]) -> bool {
    if pem.is_empty() || pem.len() > MAX_PEM_BYTES || pem.contains('\0') {
        return false;
    }
    let trimmed = pem.trim();
    let Some(label) = allowed_labels
        .iter()
        .find(|label| trimmed.starts_with(&format!("-----BEGIN {label}-----")))
    else {
        return false;
    };
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    if !trimmed.ends_with(&end)
        || trimmed.matches("-----BEGIN ").count() != 1
        || trimmed.matches("-----END ").count() != 1
    {
        return false;
    }
    let Some(body) = trimmed
        .strip_prefix(&begin)
        .and_then(|rest| rest.strip_suffix(&end))
    else {
        return false;
    };
    let body: String = body
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    !body.is_empty() && STANDARD.decode(body).is_ok()
}

fn valid_material(certificate_pem: &str, private_key_pem: &str) -> bool {
    valid_pem(certificate_pem, &["CERTIFICATE"])
        && valid_pem(
            private_key_pem,
            &[
                "PRIVATE KEY",
                "RSA PRIVATE KEY",
                "EC PRIVATE KEY",
                "DSA PRIVATE KEY",
            ],
        )
}

fn pem_der(pem: &str) -> Result<Vec<u8>, String> {
    let trimmed = pem.trim();
    let begin = "-----BEGIN CERTIFICATE-----";
    let end = "-----END CERTIFICATE-----";
    let body = trimmed
        .strip_prefix(begin)
        .and_then(|rest| rest.strip_suffix(end))
        .ok_or_else(|| "The macOS proxy CA certificate is invalid.".to_owned())?;
    let compact = body
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    STANDARD
        .decode(compact)
        .map_err(|_| "The macOS proxy CA certificate is invalid.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, collections::HashMap};

    #[derive(Default)]
    pub(super) struct MemoryStore(Mutex<HashMap<(String, String), String>>);

    impl CaStore for MemoryStore {
        fn get(&self, service: &str, account: &str) -> Result<Option<String>, String> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .get(&(service.to_owned(), account.to_owned()))
                .cloned())
        }

        fn set(&self, service: &str, account: &str, value: &str) -> Result<(), String> {
            self.0
                .lock()
                .unwrap()
                .insert((service.to_owned(), account.to_owned()), value.to_owned());
            Ok(())
        }

        fn delete(&self, service: &str, account: &str) -> Result<(), String> {
            self.0
                .lock()
                .unwrap()
                .remove(&(service.to_owned(), account.to_owned()));
            Ok(())
        }
    }

    /// A store whose deletions fail until `failures` reach zero, or that keeps the item.
    struct FlakyStore {
        inner: MemoryStore,
        failures: std::sync::atomic::AtomicUsize,
        keeps_items: bool,
    }

    impl CaStore for FlakyStore {
        fn get(&self, service: &str, account: &str) -> Result<Option<String>, String> {
            self.inner.get(service, account)
        }

        fn set(&self, service: &str, account: &str, value: &str) -> Result<(), String> {
            self.inner.set(service, account, value)
        }

        fn delete(&self, service: &str, account: &str) -> Result<(), String> {
            use std::sync::atomic::Ordering;
            if self
                .failures
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                    left.checked_sub(1)
                })
                .is_ok()
            {
                return Err("denied".into());
            }
            if self.keeps_items {
                return Ok(());
            }
            self.inner.delete(service, account)
        }
    }

    fn lock_entries() -> usize {
        item_locks().len()
    }

    fn generated(label: &str) -> CaMaterial {
        CaMaterial::from_pem(
            format!("-----BEGIN CERTIFICATE-----\n{label}\n-----END CERTIFICATE-----\n"),
            format!("-----BEGIN PRIVATE KEY-----\n{label}\n-----END PRIVATE KEY-----\n"),
        )
        .unwrap()
    }

    #[test]
    fn get_or_create_stores_once_and_returns_the_stable_pair() {
        let store = MemoryStore::default();
        let calls = Cell::new(0);
        let first = get_or_create_with(Channel::Development, "computer-a", &store, || {
            calls.set(calls.get() + 1);
            Ok(generated("Zmlyc3Q="))
        })
        .unwrap();
        assert_eq!(calls.get(), 1);
        assert!(first.certificate_pem().contains("Zmlyc3Q="));

        let second = get_or_create_with(Channel::Development, "computer-a", &store, || {
            calls.set(calls.get() + 1);
            Ok(generated("c2Vjb25k"))
        })
        .unwrap();
        assert_eq!(calls.get(), 1);
        assert!(second.certificate_pem().contains("Zmlyc3Q="));
        assert!(second.private_key_pem().contains("Zmlyc3Q="));
        assert_eq!(first.certificate_fingerprint().unwrap().len(), 64);
    }

    #[test]
    fn get_existing_reads_a_persisted_ca_without_creating_missing_state() {
        let store = MemoryStore::default();
        assert!(
            get_existing_with(Channel::Development, "computer-a", &store)
                .unwrap()
                .is_none()
        );
        assert!(store.0.lock().unwrap().is_empty());

        get_or_create_with(Channel::Development, "computer-a", &store, || {
            Ok(generated("cGVyc2lzdGVk"))
        })
        .unwrap();
        let persisted = get_existing_with(Channel::Development, "computer-a", &store)
            .unwrap()
            .unwrap();
        assert!(persisted.certificate_pem().contains("cGVyc2lzdGVk"));
    }

    #[test]
    fn channel_and_computer_ids_have_separate_keychain_items() {
        let store = MemoryStore::default();
        for (channel, id, marker) in [
            (Channel::Production, "computer-a", "cHJvZA=="),
            (Channel::Development, "computer-a", "ZGV2"),
            (Channel::Development, "computer-b", "Y29tcHV0ZXItYg=="),
        ] {
            let material =
                get_or_create_with(channel, id, &store, || Ok(generated(marker))).unwrap();
            assert!(material.certificate_pem().contains(marker));
        }
        assert_eq!(store.0.lock().unwrap().len(), 3);
        assert_ne!(
            service_name(Channel::Production),
            service_name(Channel::Development)
        );
        assert_eq!(service_name(Channel::Production), "org.silo.Silo.proxy-ca");
        assert_eq!(service_name(Channel::Development), "org.silo.dev.proxy-ca");
    }

    #[test]
    fn failed_generation_and_invalid_pem_leave_the_store_empty() {
        let store = MemoryStore::default();
        let error = get_or_create_with(Channel::Development, "computer-a", &store, || {
            Err("private diagnostic with PEM content".into())
        })
        .err()
        .expect("generation should fail");
        assert_eq!(error, "Could not generate the macOS proxy CA.");

        let error = get_or_create_with(Channel::Development, "computer-a", &store, || {
            Ok(CaMaterial {
                certificate_pem: "not a certificate".into(),
                private_key_pem: "not a key".into(),
            })
        })
        .err()
        .expect("saved data should be rejected");
        assert_eq!(error, "The macOS proxy CA PEM material is invalid.");
        assert!(store.0.lock().unwrap().is_empty());
    }

    #[test]
    fn malformed_saved_material_fails_closed_without_regeneration() {
        let store = MemoryStore::default();
        let service = service_name(Channel::Development);
        let account = account_name("computer-a");
        store.0.lock().unwrap().insert(
            (service, account),
            r#"{"version":1,"certificatePem":"bad","privateKeyPem":"bad"}"#.into(),
        );
        let generator_called = Cell::new(false);
        let error = get_or_create_with(Channel::Development, "computer-a", &store, || {
            generator_called.set(true);
            Ok(generated("bmV3"))
        })
        .err()
        .expect("unverified storage should fail");
        assert_eq!(error, "The stored macOS proxy CA is invalid.");
        assert!(!generator_called.get());
    }

    #[test]
    fn writes_are_read_back_before_material_is_returned() {
        struct NoReadback;
        impl CaStore for NoReadback {
            fn get(&self, _service: &str, _account: &str) -> Result<Option<String>, String> {
                Ok(None)
            }

            fn set(&self, _service: &str, _account: &str, _value: &str) -> Result<(), String> {
                Ok(())
            }

            fn delete(&self, _service: &str, _account: &str) -> Result<(), String> {
                Ok(())
            }
        }

        let error = get_or_create_with(Channel::Development, "computer-a", &NoReadback, || {
            Ok(generated("cGVyc2lzdGVk"))
        })
        .err()
        .expect("unverified storage should fail");
        assert!(error.contains("could not be verified"));
    }

    #[test]
    fn computer_ids_are_bounded_and_safe_for_keychain_accounts() {
        let store = MemoryStore::default();
        let too_long = "x".repeat(65);
        for invalid in ["", "../computer", "computer id", too_long.as_str()] {
            assert!(
                get_or_create_with(Channel::Development, invalid, &store, || {
                    Ok(generated("dmFsaWQ="))
                })
                .is_err()
            );
        }
        assert!(store.0.lock().unwrap().is_empty());
    }

    #[test]
    fn retiring_a_computer_removes_its_ca_key_and_only_its_own() {
        let store = MemoryStore::default();
        for id in ["computer-a", "computer-b"] {
            get_or_create_with(Channel::Development, id, &store, || Ok(generated("a2V5"))).unwrap();
        }
        retire_with(Channel::Development, "computer-a", &store).unwrap();
        assert!(
            get_existing_with(Channel::Development, "computer-a", &store)
                .unwrap()
                .is_none()
        );
        assert!(
            get_existing_with(Channel::Development, "computer-b", &store)
                .unwrap()
                .is_some()
        );
        // A computer that never had a CA, and a repeat, are not failures.
        retire_with(Channel::Development, "computer-a", &store).unwrap();
        retire_with(Channel::Development, "never-staged", &store).unwrap();
        // Another channel's item with the same computer id is untouched.
        get_or_create_with(Channel::Production, "computer-c", &store, || {
            Ok(generated("cHJvZA=="))
        })
        .unwrap();
        retire_with(Channel::Development, "computer-c", &store).unwrap();
        assert!(get_existing_with(Channel::Production, "computer-c", &store)
            .unwrap()
            .is_some());
        assert!(retire_with(Channel::Development, "../escape", &store).is_err());
    }

    #[test]
    fn a_failed_retirement_keeps_the_item_visible_and_can_be_repeated() {
        let store = FlakyStore {
            inner: MemoryStore::default(),
            failures: 1.into(),
            keeps_items: false,
        };
        get_or_create_with(Channel::Development, "computer-a", &store, || {
            Ok(generated("a2V5"))
        })
        .unwrap();
        assert!(retire_with(Channel::Development, "computer-a", &store).is_err());
        assert!(
            get_existing_with(Channel::Development, "computer-a", &store)
                .unwrap()
                .is_some()
        );
        retire_with(Channel::Development, "computer-a", &store).unwrap();
        assert!(store.inner.0.lock().unwrap().is_empty());

        // A store that reports success but keeps the item is not a retirement.
        let lying = FlakyStore {
            inner: MemoryStore::default(),
            failures: 0.into(),
            keeps_items: true,
        };
        get_or_create_with(Channel::Development, "computer-b", &lying, || {
            Ok(generated("a2V5"))
        })
        .unwrap();
        assert!(retire_with(Channel::Development, "computer-b", &lying)
            .unwrap_err()
            .contains("could not be removed"));
    }

    #[test]
    fn repeated_creation_and_deletion_leaves_no_keys_and_no_lock_entries() {
        let store = MemoryStore::default();
        let before = lock_entries();
        for round in 0..64 {
            let id = format!("repeat-{round}");
            get_or_create_with(Channel::Development, &id, &store, || Ok(generated("a2V5")))
                .unwrap();
            get_existing_with(Channel::Development, &id, &store).unwrap();
            retire_with(Channel::Development, &id, &store).unwrap();
        }
        assert!(store.0.lock().unwrap().is_empty());
        // Other tests share the process-wide map, but none of these 64 entries remain.
        let leftover = item_locks()
            .keys()
            .filter(|(_, account)| account.starts_with("computer:repeat-"))
            .count();
        assert_eq!(leftover, 0);
        assert!(
            lock_entries() <= before + 8,
            "the lock map did not grow with computers"
        );
    }

    #[test]
    fn a_held_lock_survives_pruning_until_its_holder_is_done() {
        let service = "org.silo.dev.proxy-ca";
        let held = item_lock(service, "computer:held-lock");
        let same = item_lock(service, "computer:held-lock");
        assert!(Arc::ptr_eq(&held, &same));
        prune_item_locks();
        assert!(Arc::ptr_eq(
            &held,
            &item_lock(service, "computer:held-lock")
        ));
        drop((held, same));
        prune_item_locks();
        assert!(item_locks()
            .keys()
            .all(|(_, account)| account != "computer:held-lock"));
    }
}
