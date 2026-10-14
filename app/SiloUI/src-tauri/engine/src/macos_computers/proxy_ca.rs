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
    sync::{Arc, Mutex, OnceLock, PoisonError},
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
            Err(_) => {
                Err("Cannot read the macOS proxy CA from the system credential store.".into())
            }
        }
    }

    fn set(&self, service: &str, account: &str, value: &str) -> Result<(), String> {
        keyring::Entry::new(service, account)
            .and_then(|entry| entry.set_password(value))
            .map_err(|_| "Cannot save the macOS proxy CA in the system credential store.".into())
    }
}

type ItemLocks = HashMap<(String, String), Arc<Mutex<()>>>;
static ITEM_LOCKS: OnceLock<Mutex<ItemLocks>> = OnceLock::new();

fn item_lock(service: &str, account: &str) -> Arc<Mutex<()>> {
    ITEM_LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry((service.to_owned(), account.to_owned()))
        .or_default()
        .clone()
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
        &KeychainStore,
        generate,
    )
}

/// Read the channel-scoped CA without creating or replacing Keychain state.
/// Ordinary Start uses this to fail closed when setup did not persist the CA.
pub(super) fn get_existing(computer_id: &str) -> Result<Option<CaMaterial>, String> {
    get_existing_with(crate::channel::current(), computer_id, &KeychainStore)
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
    struct MemoryStore(Mutex<HashMap<(String, String), String>>);

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
}
