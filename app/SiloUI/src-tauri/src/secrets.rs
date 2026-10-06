//! Host-only secret values. The durable document contains references and public status only.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs::{self, File},
    io::Read,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, MutexGuard, OnceLock, PoisonError, TryLockError,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

static PATH: OnceLock<PathBuf> = OnceLock::new();
static OPERATION: Mutex<()> = Mutex::new(());
static DOCUMENT: Mutex<()> = Mutex::new(());
static REMOVALS: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());
type Vault = BTreeMap<String, String>;
/// The credential-store result and when it was obtained. A failure is cached only
/// briefly so a locked or denied store does not fail every later computer start until
/// the user edits a secret; the store is asked again after `STORE_RETRY_AFTER`.
type Cached = Option<(Result<Vault, String>, Instant)>;
static VAULT: Mutex<Cached> = Mutex::new(None);
const STORE_RETRY_AFTER: Duration = Duration::from_secs(10);
const MAX_DOCUMENT_BYTES: u64 = 2 * 1024 * 1024;
fn expire_failure(cached: &mut Cached, now: Instant) {
    if matches!(cached, Some((Err(_), at)) if now.saturating_duration_since(*at) >= STORE_RETRY_AFTER)
    {
        *cached = None;
    }
}
/// `OPERATION` and `DOCUMENT` guard no data, so a panic while holding them leaves
/// nothing inconsistent: recover the guard instead of failing until restart.
fn lock_unit(mutex: &'static Mutex<()>) -> MutexGuard<'static, ()> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
fn try_lock_unit(mutex: &'static Mutex<()>) -> Option<MutexGuard<'static, ()>> {
    match mutex.try_lock() {
        Ok(guard) => Some(guard),
        Err(TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    }
}
/// A panic during a vault access may leave a partial cache; drop it so the next
/// access reloads from the credential store.
fn lock_vault() -> MutexGuard<'static, Cached> {
    VAULT.lock().unwrap_or_else(|poisoned| {
        let mut cached = poisoned.into_inner();
        *cached = None;
        VAULT.clear_poison();
        cached
    })
}
const STORE_ERROR: &str =
    "Cannot access secrets in the system credential store. Unlock it and retry.";
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Secret {
    id: String,
    name: String,
    value_id: String,
    computers: Vec<String>,
    allowed_domains: Vec<String>,
    #[serde(default)]
    affected: Vec<String>,
    #[serde(default)]
    pending_computers: Vec<String>,
    #[serde(default)]
    errors: BTreeMap<String, String>,
    #[serde(default)]
    removing: bool,
}
/// A removed generation's possible access in one guest. Never stores a value.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PendingRevocation {
    secret_id: String,
    generation: String,
    pub(crate) name: String,
    pub(crate) computer: String,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    #[serde(default)]
    pending_revocations: Vec<PendingRevocation>,
    #[serde(default)]
    secrets: Vec<Secret>,
    #[serde(default)]
    activities: Vec<Value>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    operation: String,
    id: Option<String>,
    name: String,
    value: Option<String>,
    computers: Vec<String>,
    allowed_domains: Vec<String>,
}
fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(
        crate::channel::current().keychain_service(crate::channel::Keychain::Secrets),
        "values",
    )
    .map_err(|_| STORE_ERROR.into())
}
fn read_vault() -> Result<Vault, String> {
    #[cfg(test)]
    if let Some(values) = TEST_VAULT.with(|vault| vault.borrow().clone()) {
        return Ok(values);
    }
    {
        let mut cached = lock_vault();
        expire_failure(&mut cached, Instant::now());
        if let Some((result, _)) = cached.as_ref() {
            return result.clone();
        }
    }
    // Ask the store without holding the cache lock: a macOS Keychain prompt can wait
    // indefinitely, and other readers must not queue behind it.
    let result = entry().and_then(|entry| match entry.get_password() {
        Ok(value) => serde_json::from_str(&value).map_err(|_| STORE_ERROR.into()),
        Err(keyring::Error::NoEntry) => Ok(Vault::new()),
        Err(_) => Err(STORE_ERROR.into()),
    });
    // A write or another read that finished meanwhile is at least as recent; keep it.
    lock_vault()
        .get_or_insert_with(|| (result, Instant::now()))
        .0
        .clone()
}
fn write_vault(value: Vault) -> Result<(), String> {
    #[cfg(test)]
    if TEST_VAULT.with(|vault| vault.borrow().is_some()) {
        TEST_VAULT.with(|vault| *vault.borrow_mut() = Some(value));
        return Ok(());
    }
    let mut cached = lock_vault();
    expire_failure(&mut cached, Instant::now());
    if let Some((Err(error), _)) = cached.as_ref() {
        return Err(error.clone());
    }
    if matches!(cached.as_ref(), Some((Ok(old), _)) if old == &value) {
        return Ok(());
    }
    let encoded = serde_json::to_string(&value).map_err(|_| STORE_ERROR)?;
    let result = entry()?
        .set_password(&encoded)
        .map_err(|_| STORE_ERROR.to_string());
    *cached = Some((result.clone().map(|_| value), Instant::now()));
    result
}
fn retry_store() {
    let mut cached = lock_vault();
    if matches!(cached.as_ref(), Some((Err(_), _))) {
        *cached = None;
    }
}
#[cfg(test)]
thread_local! {
    static TEST_PATH: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
    static TEST_VAULT: std::cell::RefCell<Option<Vault>> = const { std::cell::RefCell::new(None) };
}
/// Tests on this thread read and write `values` instead of the system credential
/// store, so they can assign secrets without touching the real Keychain.
#[cfg(test)]
pub(crate) fn use_test_vault(values: Option<BTreeMap<String, String>>) {
    TEST_VAULT.with(|vault| *vault.borrow_mut() = values);
}
/// Tests on this thread use `path` as the secret document instead of the app's.
/// Values still come from the credential store, so tests must not assign secrets
/// to a computer whose runtime material they read.
#[cfg(test)]
pub(crate) fn use_test_store(path: Option<PathBuf>) {
    TEST_PATH.with(|test| *test.borrow_mut() = path);
}
fn store_path() -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(path) = TEST_PATH.with(|test| test.borrow().clone()) {
        return Some(path);
    }
    PATH.get().cloned()
}
fn load() -> Result<Document, String> {
    load_from(store_path())
}
fn load_from(path: Option<PathBuf>) -> Result<Document, String> {
    let Some(path) = path else {
        return Ok(Document::default());
    };
    match File::open(&path) {
        Ok(file) => serde_json::from_reader(file.take(MAX_DOCUMENT_BYTES))
            .map_err(|_| "Secret settings could not be read. No settings were overwritten.".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Document::default()),
        Err(_) => Err("Secret settings could not be read.".into()),
    }
}
fn save(document: &Document) -> Result<(), String> {
    let path = store_path().ok_or("Secret storage is not initialized.")?;
    let parent = path.parent().ok_or("Secret storage is unavailable.")?;
    fs::create_dir_all(parent).map_err(|_| "Secret settings could not be saved.")?;
    let mut file = tempfile::NamedTempFile::new_in(parent)
        .map_err(|_| "Secret settings could not be saved.")?;
    serde_json::to_writer(&mut file, document)
        .map_err(|_| "Secret settings could not be saved.")?;
    if file
        .as_file()
        .metadata()
        .map_err(|_| "Secret settings could not be saved.")?
        .len()
        > MAX_DOCUMENT_BYTES
    {
        return Err("Secret settings are too large. Reduce assignments or allowed domains and retry. No settings were overwritten.".into());
    }
    file.as_file()
        .sync_all()
        .map_err(|_| "Secret settings could not be saved.")?;
    file.persist(&path)
        .map_err(|_| "Secret settings could not be saved.")?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| "Secret settings could not be saved.")?;
    Ok(())
}
fn update(f: impl FnOnce(&mut Document) -> Result<(), String>) -> Result<(), String> {
    let _guard = lock_unit(&DOCUMENT);
    let mut document = load()?;
    f(&mut document)?;
    save(&document)
}
fn prune_values() -> Result<(), String> {
    let referenced: BTreeSet<_> = load()?.secrets.into_iter().map(|s| s.value_id).collect();
    let mut values = read_vault()?;
    values.retain(|id, _| referenced.contains(id));
    write_vault(values)
}
fn public(secret: &Secret) -> Value {
    let errors = secret
        .errors
        .iter()
        .map(|(name, error)| format!("{name}: {error}"))
        .collect::<Vec<_>>();
    json!({"id":secret.id,"name":secret.name,"computers":secret.computers,"allowedDomains":secret.allowed_domains,
        "state": if secret.errors.is_empty() && secret.affected.iter().any(|computer| !secret.pending_computers.contains(computer)) {
            "applying"
        } else if secret.pending_computers.is_empty() {"active"} else {"restart-required"},
        "pendingComputers":secret.pending_computers,"removing":secret.removing,
        "error": if errors.is_empty() {Value::Null} else {json!(errors.join(" "))}})
}
pub(crate) fn snapshot() -> Result<Vec<Value>, String> {
    snapshot_from(store_path())
}
fn snapshot_from(path: Option<PathBuf>) -> Result<Vec<Value>, String> {
    Ok(load_from(path)?.secrets.iter().map(public).collect())
}
pub(crate) fn activities() -> Result<Vec<Value>, String> {
    Ok(load()?.activities)
}
pub(crate) fn runtime_material(
    computer: &str,
) -> Result<Vec<(String, String, Vec<String>)>, String> {
    let document = load()?;
    let selected: Vec<_> = document
        .secrets
        .iter()
        .filter(|s| !s.removing && s.computers.iter().any(|w| w == computer))
        .collect();
    if selected.is_empty() {
        return Ok(Vec::new());
    }
    let values = read_vault()?;
    selected
        .into_iter()
        .map(|s| {
            Ok((
                s.name.clone(),
                values.get(&s.value_id).ok_or(STORE_ERROR)?.clone(),
                s.allowed_domains.clone(),
            ))
        })
        .collect()
}

/// Copy assignment references only. Values remain in the host credential store.
pub(crate) fn fork_assignments(source: &str, target: &str) -> Result<(), String> {
    let _operation = try_lock_unit(&OPERATION)
        .ok_or_else(|| "Secret settings are busy. Retry the fork.".to_string())?;
    update(|document| {
        copy_assignment_refs(document, source, target);
        Ok(())
    })
}

fn copy_assignment_refs(document: &mut Document, source: &str, target: &str) {
    for secret in &mut document.secrets {
        if !secret.removing
            && secret.computers.iter().any(|name| name == source)
            && !secret.computers.iter().any(|name| name == target)
        {
            secret.computers.push(target.into());
        }
    }
}
fn event(document: &mut Document, title: &str, failed: bool) {
    let now = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();
    document.activities.push(json!({"id":uuid::Uuid::new_v4().to_string(),"category":"secrets","title":title,"detail":"Secret values are kept in the system credential store.","occurredAt":now,"time":now,"tone":if failed {"danger"} else {"success"},"status":"completed"}));
    if document.activities.len() > 100 {
        document.activities.remove(0);
    }
}
fn revision(document: &Document, computer: &str) -> String {
    let desired: Vec<_> = document
        .secrets
        .iter()
        .filter(|s| !s.removing && s.computers.iter().any(|w| w == computer))
        .map(|s| (&s.name, &s.value_id, &s.allowed_domains))
        .collect();
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&desired).unwrap_or_default())
    )
}
pub(crate) fn computer_revision(computer: &str) -> Result<String, String> {
    Ok(revision(&load()?, computer))
}
/// Per computer, a counter of verified starts and the secret revision they booted.
static STARTS: Mutex<BTreeMap<String, (u64, String)>> = Mutex::new(BTreeMap::new());
fn last_start(computer: &str) -> Option<(u64, String)> {
    STARTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(computer)
        .cloned()
}
pub(crate) fn computer_started(computer: &str, applied_revision: &str) -> Result<(), String> {
    // Called only after runtime verification. No operation lock: start owns the runtime lock.
    if store_path().is_none() {
        return Ok(());
    }
    update(|document| {
        if revision(document, computer) != applied_revision {
            return Ok(());
        }
        let mut starts = STARTS.lock().unwrap_or_else(PoisonError::into_inner);
        let start = starts.entry(computer.into()).or_default();
        *start = (start.0.wrapping_add(1), applied_revision.into());
        drop(starts);
        document
            .pending_revocations
            .retain(|record| record.computer != computer);
        for secret in &mut document.secrets {
            secret.pending_computers.retain(|w| w != computer);
            secret.affected.retain(|w| w != computer);
            secret.errors.remove(computer);
        }
        Ok(())
    })
}
/// Called with the computer gate held after an observed stop. Assignments and deferred
/// additions still apply on the next start; only removed generations are cleared.
pub(crate) fn computer_stopped(computer: &str) -> Result<(), String> {
    if store_path().is_none() || pending_names(computer)?.is_empty() {
        return Ok(());
    }
    update(|document| {
        document
            .pending_revocations
            .retain(|record| record.computer != computer);
        Ok(())
    })
}

pub(crate) fn computer_removed(computer: &str) -> Result<(), String> {
    if store_path().is_none() {
        return Ok(());
    }
    update(|document| {
        // Invalidate saves that validated the old computer, including after name reuse.
        let mut removals = REMOVALS.lock().unwrap_or_else(PoisonError::into_inner);
        let revision = removals.entry(computer.into()).or_default();
        *revision = revision.wrapping_add(1);
        drop(removals);
        document
            .pending_revocations
            .retain(|record| record.computer != computer);
        for secret in &mut document.secrets {
            secret.computers.retain(|w| w != computer);
            secret.affected.retain(|w| w != computer);
            secret.pending_computers.retain(|w| w != computer);
            secret.errors.remove(computer);
        }
        Ok(())
    })
}
fn assignment_revision(computers: &[String]) -> Vec<u64> {
    let removals = REMOVALS.lock().unwrap_or_else(PoisonError::into_inner);
    computers
        .iter()
        .map(|computer| removals.get(computer).copied().unwrap_or_default())
        .collect()
}
/// Called inside the document transaction so deletion cannot overtake the commit.
fn ensure_assignment_revision(computers: &[String], expected: &[u64]) -> Result<(), String> {
    if assignment_revision(computers) != expected {
        return Err("A selected computer was removed while saving this secret. Select computers again and retry.".into());
    }
    Ok(())
}
#[derive(Deserialize)]
struct ReservedSecretNames {
    names: Vec<String>,
    prefixes: Vec<String>,
}

/// The one list of names a secret may not use, shared with the UI's validation
/// (`features/application/model/reserved-secret-names.json`).
fn reserved_secret_names() -> &'static ReservedSecretNames {
    static RESERVED: OnceLock<ReservedSecretNames> = OnceLock::new();
    RESERVED.get_or_init(|| {
        serde_json::from_str(include_str!(
            "../../src/features/application/model/reserved-secret-names.json"
        ))
        .expect("the bundled reserved secret names are valid JSON")
    })
}

/// True when a secret must not use `name` (compared case-insensitively): it would
/// override guest shell, proxy, TLS or loader settings, or a Silo or runtime variable.
pub(crate) fn reserved_secret_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    let reserved = reserved_secret_names();
    reserved.names.iter().any(|reserved| *reserved == upper)
        || reserved
            .prefixes
            .iter()
            .any(|prefix| upper.starts_with(prefix.as_str()))
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .enumerate()
            .all(|(i, c)| c == b'_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit()))
        && !reserved_secret_name(name)
}
fn valid_domain(domain: &str) -> bool {
    if domain == "*" {
        return true;
    }
    let host = domain.strip_prefix("*.").unwrap_or(domain);
    host.len() <= 253
        && (!domain.starts_with("*.") || host.contains('.'))
        && host.split('.').all(|part| {
            !part.is_empty()
                && part.len() <= 63
                && !part.starts_with('-')
                && !part.ends_with('-')
                && part
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        })
}
fn validate(request: &Request, document: &Document) -> Result<(), String> {
    if !valid_name(&request.name) {
        return Err("Choose a valid, unreserved secret name.".into());
    }
    if !matches!(request.operation.as_str(), "add" | "edit") {
        return Err("Unknown secret operation.".into());
    }
    let original = request
        .id
        .as_ref()
        .and_then(|id| document.secrets.iter().find(|s| &s.id == id));
    if request.operation == "edit" && original.is_none() {
        return Err("This secret no longer exists.".into());
    }
    if original.is_some_and(|s| s.name != request.name || s.removing) {
        return Err("Secret names cannot change. Finish removal before adding it again.".into());
    }
    if document
        .secrets
        .iter()
        .any(|s| s.name == request.name && Some(&s.id) != request.id.as_ref())
    {
        return Err("A secret with this name already exists.".into());
    }
    if request.operation == "add"
        && (request.id.is_some() || request.value.as_deref().is_none_or(str::is_empty))
    {
        return Err("Enter a secret value.".into());
    }
    if request
        .value
        .as_ref()
        .is_some_and(|v| v.is_empty() || v.len() > 64 * 1024 || v.contains('\0'))
    {
        return Err(
            "Secret values must contain between 1 and 65536 bytes without null characters.".into(),
        );
    }
    if request.computers.is_empty()
        || request.computers.len() > 100
        || request.computers.iter().collect::<BTreeSet<_>>().len() != request.computers.len()
        || request.allowed_domains.is_empty()
        || request.allowed_domains.len() > 100
        || !request.allowed_domains.iter().all(|d| valid_domain(d))
    {
        return Err("Select computers and valid allowed domains.".into());
    }
    Ok(())
}
type Material = Vec<(String, String, Vec<String>)>;
type OperationGuard = Option<MutexGuard<'static, ()>>;
fn reconcile(app: &AppHandle, id: &str, operation: &mut OperationGuard) -> Result<(), String> {
    reconcile_with(
        id,
        operation,
        &runtime_material,
        &mut |computer, _desired| crate::runtime::apply_secrets(app, computer),
        &|| {
            let _ = app.emit("silo://application-state-changed", ());
        },
    )
}
/// Applies one secret's desired state to each affected computer. The global secret
/// operation lock is released while a computer applies, which can wait on that computer's gate
/// for minutes, so forks, updates and other secret operations are not blocked. It
/// is re-taken to record each result and before credential-store changes. When the
/// Computer's desired secrets changed while unlocked, the newer state is applied again.
fn reconcile_with(
    id: &str,
    operation: &mut OperationGuard,
    material: &dyn Fn(&str) -> Result<Material, String>,
    apply: &mut dyn FnMut(&str, Material) -> Result<Vec<String>, String>,
    changed: &dyn Fn(),
) -> Result<(), String> {
    update(|d| {
        let secret = d
            .secrets
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or("This secret no longer exists.")?;
        secret.errors.remove("Credential store");
        Ok(())
    })?;
    let document = load()?;
    let secret = document
        .secrets
        .iter()
        .find(|s| s.id == id)
        .ok_or("This secret no longer exists.")?;
    let targets: BTreeSet<_> = secret
        .affected
        .iter()
        .chain(secret.computers.iter())
        .cloned()
        .collect();
    for computer in targets {
        let mut attempts = 0;
        let (result, applied_revision) = loop {
            if !load()?.secrets.iter().any(|secret| secret.id == id) {
                return Ok(());
            }
            let desired_revision = computer_revision(&computer)?;
            let started_before = last_start(&computer);
            let desired = material(&computer);
            *operation = None;
            let result = desired.and_then(|desired| apply(&computer, desired));
            *operation = Some(lock_unit(&OPERATION));
            attempts += 1;
            if attempts >= 3 || computer_revision(&computer)? == desired_revision {
                // A restart that finished while this apply ran already booted with the
                // desired secrets, so a deferred result must not ask for another one.
                let restarted = last_start(&computer).is_some_and(|start| {
                    Some(&start) != started_before.as_ref() && start.1 == desired_revision
                });
                break (
                    result.map(|pending| if restarted { Vec::new() } else { pending }),
                    desired_revision,
                );
            }
        };
        update(|document| {
            // Exhausted retries and concurrent deletion cannot publish an obsolete result.
            if revision(document, &computer) != applied_revision {
                if let Some(secret) = document.secrets.iter_mut().find(|s| s.id == id) {
                    if secret.affected.contains(&computer)
                        && !secret.pending_computers.contains(&computer)
                    {
                        secret.errors.entry(computer.clone()).or_insert_with(|| {
                            "Secret settings changed during this update. Retry to verify the latest settings.".into()
                        });
                    }
                }
                return Ok(());
            }
            let Some(secret) = document.secrets.iter_mut().find(|s| s.id == id) else {
                return Ok(());
            };
            secret.pending_computers.retain(|w| w != &computer);
            match &result {
                Ok(pending_names) => {
                    secret.errors.remove(&computer);
                    if !secret.removing && pending_names.contains(&secret.name) {
                        secret.pending_computers.push(computer.clone());
                    } else {
                        secret.affected.retain(|w| w != &computer);
                    }
                }
                Err(error) => {
                    secret.errors.insert(computer.clone(), error.clone());
                }
            }
            Ok(())
        })?;
        changed();
    }
    let document = load()?;
    let Some(secret) = document.secrets.iter().find(|s| s.id == id) else {
        return Ok(());
    };
    let failed = !secret.errors.is_empty();
    update(|d| {
        event(
            d,
            if failed {
                "Secret changes could not be applied"
            } else {
                "Secret settings saved"
            },
            failed,
        );
        Ok(())
    })?;
    Ok(())
}
/// Commit the name-only retry journal before deleting the credential. A credential
/// failure leaves a retryable tombstone excluded from all future boot material.
fn remove_from_store(id: &str) -> Result<(), String> {
    let secret = load()?
        .secrets
        .into_iter()
        .find(|secret| secret.id == id)
        .ok_or("This secret no longer exists.")?;
    update(|document| {
        document
            .secrets
            .iter_mut()
            .find(|secret| secret.id == id)
            .ok_or("This secret no longer exists.")?
            .removing = true;
        for computer in secret
            .computers
            .iter()
            .chain(&secret.affected)
            .collect::<BTreeSet<_>>()
        {
            let record = PendingRevocation {
                secret_id: secret.id.clone(),
                generation: secret.value_id.clone(),
                name: secret.name.clone(),
                computer: computer.clone(),
            };
            if !document.pending_revocations.contains(&record) {
                document.pending_revocations.push(record);
            }
        }
        Ok(())
    })?;
    let mut values = read_vault()?;
    values.remove(&secret.value_id);
    write_vault(values)?;
    update(|document| {
        document.secrets.retain(|secret| secret.id != id);
        event(document, "Secret removed", false);
        Ok(())
    })
}

pub(crate) fn pending_names(computer: &str) -> Result<Vec<String>, String> {
    Ok(pending_names_by_computer()?
        .remove(computer)
        .unwrap_or_default())
}

/// Every computer's names awaiting revocation, from one reading of the settings.
pub(crate) fn pending_names_by_computer() -> Result<HashMap<String, Vec<String>>, String> {
    let mut names: HashMap<String, BTreeSet<String>> = HashMap::new();
    for record in load()?.pending_revocations {
        names
            .entry(record.computer)
            .or_default()
            .insert(record.name);
    }
    Ok(names
        .into_iter()
        .map(|(computer, names)| (computer, names.into_iter().collect()))
        .collect())
}

/// Called inside the computer gate, immediately before name-only revocation. A current
/// assignment of this name must be applied by its own reconcile, never removed by
/// a retry for an older generation. Keep the warning until replacement is verified.
pub(crate) fn revocation_needed(record: &PendingRevocation) -> Result<bool, String> {
    let document = load()?;
    Ok(document.pending_revocations.contains(record)
        && !document.secrets.iter().any(|secret| {
            !secret.removing
                && secret.name == record.name
                && secret.computers.contains(&record.computer)
        }))
}

/// A successful live update replaced old values for these names. Clear only records
/// present before that update, so a concurrent later removal remains pending.
pub(crate) fn revocations_replaced(
    records: &[PendingRevocation],
    computer: &str,
    pending_names: &[String],
) -> Result<(), String> {
    update(|document| {
        document.pending_revocations.retain(|record| {
            record.computer != computer
                || pending_names.contains(&record.name)
                || !records.contains(record)
        });
        Ok(())
    })
}

pub(crate) fn pending_revocations() -> Result<Vec<PendingRevocation>, String> {
    Ok(load()?.pending_revocations)
}

fn retry_revocations_with(
    revoke: &mut dyn FnMut(&PendingRevocation) -> Result<bool, String>,
) -> Result<bool, String> {
    let mut changed = false;
    for record in pending_revocations()? {
        if revoke(&record).unwrap_or(false) {
            update(|document| {
                document
                    .pending_revocations
                    .retain(|pending| pending != &record);
                Ok(())
            })?;
            changed = true;
        }
    }
    Ok(changed)
}

static REVOCATIONS_RUNNING: AtomicBool = AtomicBool::new(false);
/// One pass per state refresh, off the read/Remove path; busy guests are skipped.
/// Remote snapshots invoke the same owner-side read and retry path.
pub(crate) fn schedule_revocations(app: &AppHandle) {
    if pending_revocations().is_ok_and(|records| records.is_empty())
        || REVOCATIONS_RUNNING.swap(true, Ordering::AcqRel)
    {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        struct Running;
        impl Drop for Running {
            fn drop(&mut self) {
                REVOCATIONS_RUNNING.store(false, Ordering::Release);
            }
        }
        let _running = Running;
        let Ok(_update) = crate::updates::operation_guard() else {
            return;
        };
        if retry_revocations_with(&mut |record| crate::runtime::revoke_secret(&app, record))
            .unwrap_or(false)
        {
            let _ = app.emit("silo://application-state-changed", ());
        }
    });
}

fn require_main(window: &WebviewWindow) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("Secret changes are only available in the main window.".into())
    }
}
#[tauri::command]
pub async fn read_secrets_state() -> Result<Vec<Value>, String> {
    let path = store_path();
    tauri::async_runtime::spawn_blocking(move || snapshot_from(path))
        .await
        .map_err(|_| "Secret settings could not be read.".to_string())?
}
#[tauri::command]
pub async fn save_secret(
    app: AppHandle,
    window: WebviewWindow,
    request: Request,
) -> Result<Vec<Value>, String> {
    require_main(&window)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _update = crate::updates::operation_guard()?;
        let mut operation = Some(lock_unit(&OPERATION));
        retry_store();
        let validated_assignments = assignment_revision(&request.computers);
        let document = load()?;
        validate(&request, &document)?;
        crate::runtime::validate_secret_computers(&app, &request.computers)?;
        let id = request
            .id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let original = document.secrets.iter().find(|s| s.id == id);
        let value_id = if let Some(value) = &request.value {
            let value_id = uuid::Uuid::new_v4().to_string();
            let mut values = read_vault()?;
            values.insert(value_id.clone(), value.clone());
            write_vault(values)?;
            value_id
        } else {
            original.ok_or("Enter a secret value.")?.value_id.clone()
        };
        update(|d| {
            ensure_assignment_revision(&request.computers, &validated_assignments)?;
            let affected = original
                .into_iter()
                .flat_map(|s| s.affected.iter().chain(s.computers.iter()))
                .chain(request.computers.iter())
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            d.secrets.retain(|s| s.id != id);
            d.secrets.push(Secret {
                id: id.clone(),
                name: request.name,
                value_id,
                computers: request.computers,
                allowed_domains: request.allowed_domains,
                affected,
                pending_computers: Vec::new(),
                errors: BTreeMap::new(),
                removing: false,
            });
            Ok(())
        })?;
        let _ = app.emit("silo://application-state-changed", ());
        reconcile(&app, &id, &mut operation)?;
        let _ = prune_values();
        snapshot()
    })
    .await
    .map_err(|_| "Secret changes could not finish.".to_string())?
}
#[tauri::command]
pub async fn remove_secret(
    app: AppHandle,
    window: WebviewWindow,
    id: String,
) -> Result<Vec<Value>, String> {
    require_main(&window)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _update = crate::updates::operation_guard()?;
        let _operation = lock_unit(&OPERATION);
        retry_store();
        remove_from_store(&id)?;
        let _ = app.emit("silo://application-state-changed", ());
        schedule_revocations(&app);
        snapshot()
    })
    .await
    .map_err(|_| "Secret removal could not finish.".to_string())?
}
#[tauri::command]
pub async fn retry_secret(
    app: AppHandle,
    window: WebviewWindow,
    id: String,
) -> Result<Vec<Value>, String> {
    require_main(&window)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _update = crate::updates::operation_guard()?;
        let mut operation = Some(lock_unit(&OPERATION));
        retry_store();
        if load()?
            .secrets
            .iter()
            .any(|secret| secret.id == id && secret.removing)
        {
            remove_from_store(&id)?;
            schedule_revocations(&app);
        } else {
            reconcile(&app, &id, &mut operation)?;
        }
        let _ = prune_values();
        snapshot()
    })
    .await
    .map_err(|_| "Secret changes could not finish.".to_string())?
}
pub(crate) fn install(app: &AppHandle) -> Result<(), String> {
    PATH.set(
        app.path()
            .app_data_dir()
            .map_err(|_| "Secret storage is unavailable.")?
            .join("secrets.json"),
    )
    .ok();
    let app = app.clone();
    std::thread::spawn(move || {
        let mut operation = Some(lock_unit(&OPERATION));
        if let Ok(document) = load() {
            // Migrate legacy removals before any slow live update can wait on a computer.
            for secret in document.secrets.iter().filter(|secret| secret.removing) {
                let _ = remove_from_store(&secret.id);
            }
            for secret in document
                .secrets
                .iter()
                .filter(|secret| !secret.removing && !secret.affected.is_empty())
            {
                let _ = reconcile(&app, &secret.id, &mut operation);
            }
        }
        drop(operation);
        schedule_revocations(&app);
        let _ = app.emit("silo://application-state-changed", ());
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrated_secret_settings_load() {
        let migrated = crate::runtime_migration::vocabulary_tests::migrated_installation();
        let document = load_from(Some(migrated.app_data.join("secrets.json"))).unwrap();
        assert_eq!(document.secrets.len(), 1);
        assert_eq!(document.secrets[0].computers, ["dev"]);
        assert_eq!(document.secrets[0].pending_computers, ["dev"]);
        assert_eq!(document.pending_revocations[0].computer, "dev");
    }

    #[cfg(unix)]
    #[test]
    fn secret_document_save_reports_an_unreadable_parent_after_publication() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let directory = tempfile::tempdir().unwrap();
        if fs::metadata(directory.path()).unwrap().uid() == 0 {
            return; // Root bypasses the permission boundary exercised here.
        }
        let path = directory.path().join("secrets.json");
        use_test_store(Some(path.clone()));
        let document = Document {
            activities: vec![serde_json::json!({"title": "fixture change"})],
            ..Default::default()
        };
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o300)).unwrap();
        let result = save(&document);
        use_test_store(None);
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let published: Document = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(published.activities, document.activities);
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        assert!(
            result.is_err(),
            "an unsynchronized rename must not report success"
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
        use_test_store(Some(path));
        let retry = save(&document);
        use_test_store(None);
        assert!(retry.is_ok());
    }
    fn request() -> Request {
        Request {
            operation: "add".into(),
            id: None,
            name: "API_KEY".into(),
            value: Some("private-value".into()),
            computers: vec!["dev".into()],
            allowed_domains: vec!["api.example.com".into()],
        }
    }
    fn secret() -> Secret {
        Secret {
            id: "id".into(),
            name: "API_KEY".into(),
            value_id: "private-reference".into(),
            computers: vec!["dev".into()],
            allowed_domains: vec!["api.example.com".into()],
            affected: vec!["dev".into()],
            pending_computers: Vec::new(),
            errors: BTreeMap::new(),
            removing: false,
        }
    }
    #[cfg(unix)]
    #[test]
    fn slow_secret_read_keeps_the_async_executor_responsive() {
        use std::io::Write;

        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        let fifo = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        use_test_store(Some(path.clone()));
        let (heartbeat, observed) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            let responsive = observed.recv_timeout(Duration::from_secs(2)).is_ok();
            File::create(path).unwrap().write_all(b"{}").unwrap();
            responsive
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let (result, ()) = runtime.block_on(async {
            tokio::join!(biased; read_secrets_state(), async {
                let _ = heartbeat.send(());
            })
        });
        use_test_store(None);
        assert_eq!(result.unwrap(), Vec::<Value>::new());
        assert!(
            writer.join().unwrap(),
            "the secret read blocked the executor heartbeat"
        );
    }
    #[test]
    fn oversized_save_preserves_readable_settings() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        use_test_store(Some(path.clone()));
        let domain = format!(
            "{}.{}.{}.{}",
            "a".repeat(63),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(61)
        );
        let mut document = Document::default();
        for index in 0..90 {
            let mut request = request();
            request.name = format!("TOKEN_{index}");
            request.allowed_domains = vec![domain.clone(); 100];
            validate(&request, &document).unwrap();
            let mut entry = secret();
            entry.id = format!("secret-{index}");
            entry.name = request.name;
            entry.allowed_domains = request.allowed_domains;
            document.secrets.push(entry);
            if index == 74 {
                save(&document).unwrap();
            }
        }
        let previous = fs::read(&path).unwrap();
        assert!(previous.len() < 2 * 1024 * 1024);
        assert!(serde_json::to_vec(&document).unwrap().len() > 2 * 1024 * 1024);
        assert!(save(&document).is_err());
        assert_eq!(fs::read(&path).unwrap(), previous);
        assert_eq!(load().unwrap().secrets.len(), 75);
        update(|document| {
            document.secrets.pop();
            Ok(())
        })
        .unwrap();
        assert_eq!(load().unwrap().secrets.len(), 74);
        use_test_store(None);
    }
    #[test]
    fn fork_copies_current_assignment_reference_without_copying_value() {
        let _test_state = crate::test_support::global_state();
        let mut document = Document {
            secrets: vec![secret()],
            activities: Vec::new(),
            ..Default::default()
        };
        copy_assignment_refs(&mut document, "dev", "fork");
        assert_eq!(document.secrets[0].computers, ["dev", "fork"]);
        assert_eq!(document.secrets[0].value_id, "private-reference");
        document.secrets[0].computers.retain(|name| name != "dev");
        assert_eq!(document.secrets[0].computers, ["fork"]);
    }
    #[test]
    fn applied_revision_changes_for_rotation_domains_and_removal_not_status() {
        let _test_state = crate::test_support::global_state();
        let mut d = Document {
            secrets: vec![secret()],
            ..Default::default()
        };
        let original = revision(&d, "dev");
        d.secrets[0].errors.insert("dev".into(), "Retry".into());
        d.secrets[0].pending_computers.push("dev".into());
        assert_eq!(revision(&d, "dev"), original);
        d.secrets[0].value_id = "new-value-reference".into();
        assert_ne!(revision(&d, "dev"), original);
        let rotated = revision(&d, "dev");
        d.secrets[0].allowed_domains = vec!["other.example.com".into()];
        assert_ne!(revision(&d, "dev"), rotated);
        d.secrets[0].removing = true;
        assert_eq!(revision(&d, "dev"), revision(&Document::default(), "dev"));
    }
    #[test]
    fn backend_rejects_reserved_names_before_storage() {
        let _test_state = crate::test_support::global_state();
        for name in [
            "PATH",
            "HOME",
            "GH_TOKEN",
            "SILO_GITHUB",
            "MSB_HOME",
            "LD_PRELOAD",
            "DYLD_INSERT_LIBRARIES",
            "http_proxy",
            "1KEY",
            "A=B",
            "msb_path",
        ] {
            let mut r = request();
            r.name = name.into();
            assert!(validate(&r, &Document::default()).is_err(), "{name}");
        }
        assert!(validate(&request(), &Document::default()).is_ok());
    }
    #[test]
    fn saving_and_applying_secrets_share_one_reserved_name_list() {
        let _test_state = crate::test_support::global_state();
        assert!(
            !reserved_secret_names().names.is_empty()
                && !reserved_secret_names().prefixes.is_empty()
        );
        for name in ["no_proxy", "Path", "silo_anything", "RUST_LOG"] {
            assert!(reserved_secret_name(name), "{name}");
            // The runtime refuses the same names even if a saved document contained them.
            let material = vec![(
                name.to_string(),
                "value".to_string(),
                vec!["api.example.com".to_string()],
            )];
            assert!(
                crate::runtime::validate_secret_material_for_tests(&material).is_err(),
                "{name}"
            );
        }
        assert!(!reserved_secret_name("API_KEY"));
    }
    #[test]
    fn values_and_domain_constraints_are_validated_on_host() {
        let _test_state = crate::test_support::global_state();
        for value in [String::new(), "bad\0value".into(), "a".repeat(65537)] {
            let mut r = request();
            r.value = Some(value);
            assert!(validate(&r, &Document::default()).is_err());
        }
        for domain in [
            "https://api.example.com",
            "api.example.com/path",
            "user@host.test",
            "*.com",
            "api.example.com:443",
            "-bad.test",
            "",
        ] {
            assert!(!valid_domain(domain), "{domain}");
        }
        for domain in ["api.example.com", "*.example.com", "*"] {
            assert!(valid_domain(domain));
        }
    }
    /// Owner decision 4: choosing allowed domains is the user's responsibility.
    /// Validation only checks syntax; it does not consult the public suffix list.
    #[test]
    fn allowed_domain_syntax_intent_matches_owner_decision_four() {
        let _test_state = crate::test_support::global_state();
        for accepted in [
            "*",       // explicit opt-in to every domain
            "*.co.uk", // public-suffix wildcards are allowed by decision
            "*.github.io",
            "*.vercel.app",
            "localhost", // single-label hosts are allowed
            "127.0.0.1", // IPv4 literals parse as numeric labels
            "10.0.0.1",
            "xn--bcher-kva.example", // punycode labels
            "a-b.example.com",
        ] {
            assert!(valid_domain(accepted), "{accepted} should be accepted");
        }
        for rejected in [
            "*.com", // a wildcard needs at least two labels
            "*.xn--p1ai",
            "example.com.", // trailing dot: use the name without it
            ".example.com",
            "API.example.com", // names must be entered in lower case
            "*.*.example.com", // only one leading wildcard label
            "a*.example.com",
            "*example.com",
            "::1", // IPv6 literals are not supported
            "[::1]",
            "exa mple.com",
            "b\u{fc}cher.example", // use punycode for internationalized names
            &format!("{}.com", "a".repeat(64)),
            &format!("{}.com", ["a"; 127].join(".")),
        ] {
            assert!(!valid_domain(rejected), "{rejected} should be rejected");
        }
    }
    #[test]
    fn edits_preserve_name_and_do_not_require_value() {
        let _test_state = crate::test_support::global_state();
        let mut r = request();
        r.operation = "edit".into();
        r.id = Some("id".into());
        r.value = None;
        let d = Document {
            secrets: vec![secret()],
            ..Default::default()
        };
        assert!(validate(&r, &d).is_ok());
        r.name = "NEW_KEY".into();
        assert!(validate(&r, &d).is_err());
        assert!(validate(&request(), &d).is_err());
    }
    #[test]
    fn snapshots_expose_only_public_metadata_and_actual_pending_computers() {
        let _test_state = crate::test_support::global_state();
        let mut s = secret();
        s.pending_computers = vec!["dev".into()];
        let value = public(&s);
        let text = value.to_string();
        assert!(!text.contains("private-reference"));
        assert!(value.get("value").is_none());
        assert_eq!(value["state"], "restart-required");
        assert_eq!(value["pendingComputers"], json!(["dev"]));
        s.pending_computers.clear();
        s.errors
            .insert("dev".into(), "Could not apply secret changes.".into());
        assert!(public(&s)["error"].as_str().unwrap().contains("dev:"));
        s.removing = true;
        assert_eq!(public(&s)["removing"], true);
    }
    #[test]
    fn persisted_unfinished_secret_targets_are_not_reported_active_after_relaunch() {
        let _test_state = crate::test_support::global_state();
        let mut secret = secret();
        assert_eq!(public(&secret)["state"], "applying");
        secret.pending_computers.push("dev".into());
        assert_eq!(public(&secret)["state"], "restart-required");
        secret.affected.push("other".into());
        assert_eq!(public(&secret)["state"], "applying");
        secret
            .errors
            .insert("other".into(), "Could not apply changes.".into());
        assert_eq!(public(&secret)["state"], "restart-required");
        assert!(public(&secret)["error"].is_string());
        secret.affected.clear();
        secret.pending_computers.clear();
        secret.errors.clear();
        assert_eq!(public(&secret)["state"], "active");
    }
    #[test]
    fn poisoned_locks_recover_instead_of_blocking_secrets_and_updates() {
        let _test_state = crate::test_support::global_state();
        static TEST: Mutex<()> = Mutex::new(());
        let _ = std::thread::spawn(|| {
            let _guard = TEST.lock().unwrap();
            panic!("poison");
        })
        .join();
        assert!(TEST.is_poisoned());
        drop(lock_unit(&TEST));
        let held = try_lock_unit(&TEST).expect("poisoned lock is recoverable");
        assert!(try_lock_unit(&TEST).is_none());
        drop(held);
    }
    #[test]
    fn cached_store_failure_expires_so_later_starts_ask_the_store_again() {
        let _test_state = crate::test_support::global_state();
        let failed_at = Instant::now();
        let mut cached: Cached = Some((Err(STORE_ERROR.into()), failed_at));
        expire_failure(&mut cached, failed_at + Duration::from_secs(1));
        assert!(
            cached.is_some(),
            "a fresh failure is not retried in a tight loop"
        );
        expire_failure(&mut cached, failed_at + STORE_RETRY_AFTER);
        assert!(
            cached.is_none(),
            "an old failure no longer blocks computer starts"
        );
        let mut unlocked: Cached = Some((Ok(Vault::new()), failed_at));
        expire_failure(&mut unlocked, failed_at + STORE_RETRY_AFTER * 100);
        assert!(unlocked.is_some(), "successful reads stay cached");
    }
    #[test]
    fn deleting_and_recreating_a_computer_leaves_it_no_secret_material() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        use_test_store(Some(dir.path().join("secrets.json")));
        let mut assigned = secret();
        assigned.computers = vec!["dev".into(), "other".into()];
        assigned.pending_computers = vec!["dev".into()];
        assigned.errors.insert("dev".into(), "Retry".into());
        save(&Document {
            secrets: vec![assigned],
            activities: Vec::new(),
            ..Default::default()
        })
        .unwrap();
        computer_removed("dev").unwrap();
        let document = load().unwrap();
        let kept = &document.secrets[0];
        assert_eq!(kept.computers, ["other"]);
        assert!(kept.affected.is_empty() && kept.pending_computers.is_empty());
        assert!(kept.errors.is_empty());
        // A new `dev` selects no secrets, so no credential-store read happens.
        assert!(runtime_material("dev").unwrap().is_empty());
        assert_eq!(
            computer_revision("dev").unwrap(),
            revision(&Document::default(), "dev")
        );
        use_test_store(None);
    }
    #[test]
    fn deletion_does_not_wait_for_an_update_secret_guard() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        use_test_store(Some(path.clone()));
        save(&Document::default()).unwrap();
        let operation = update_guard().unwrap();
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        let deletion = std::thread::spawn(move || {
            use_test_store(Some(path));
            let result = computer_removed("dev");
            finished_tx.send(()).unwrap();
            use_test_store(None);
            result
        });
        let completed = finished_rx.recv_timeout(Duration::from_secs(1)).is_ok();
        drop(operation);
        deletion.join().unwrap().unwrap();
        assert!(
            completed,
            "inventory cleanup cannot wait behind an updater's secret lock"
        );
        use_test_store(None);
    }
    #[test]
    fn deletion_rejects_an_already_validated_save_even_after_name_reuse() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        use_test_store(Some(path.clone()));
        save(&Document {
            secrets: vec![secret()],
            ..Default::default()
        })
        .unwrap();
        let save_operation = lock_unit(&OPERATION);
        let original = load().unwrap().secrets.remove(0);
        let validated_assignments = assignment_revision(&original.computers);
        computer_removed("other").unwrap();
        ensure_assignment_revision(&original.computers, &validated_assignments).unwrap();
        // The save is waiting on its credential store while the old computer is deleted.
        computer_removed("dev").unwrap();
        let committed = update(|document| {
            ensure_assignment_revision(&original.computers, &validated_assignments)?;
            document.secrets.clear();
            document.secrets.push(original.clone());
            Ok(())
        });
        drop(save_operation);
        assert!(committed.unwrap_err().contains("was removed"));
        let document = load().unwrap();
        assert!(document.secrets[0].computers.is_empty());
        assert!(document.secrets[0].affected.is_empty());
        // A replacement computer with this name selects no material or credential values.
        assert!(runtime_material("dev").unwrap().is_empty());
        // Only a fresh save validated after name reuse can assign to the replacement.
        let replacement_assignments = assignment_revision(&original.computers);
        update(|document| {
            ensure_assignment_revision(&original.computers, &replacement_assignments)?;
            document.secrets.clear();
            document.secrets.push(original);
            Ok(())
        })
        .unwrap();
        assert_eq!(load().unwrap().secrets[0].computers, ["dev"]);
        use_test_store(None);
    }
    #[test]
    fn exhausted_reconcile_keeps_retry_available_for_an_unapplied_revision() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        use_test_store(Some(dir.path().join("secrets.json")));
        save(&Document {
            secrets: vec![secret()],
            ..Default::default()
        })
        .unwrap();
        let mut operation = Some(lock_unit(&OPERATION));
        let mut attempts = 0;
        reconcile_with(
            "id",
            &mut operation,
            &|_| Ok(Vec::new()),
            &mut |_, _| {
                attempts += 1;
                update(|document| {
                    document.secrets[0].value_id = format!("new-generation-{attempts}");
                    Ok(())
                })?;
                Ok(Vec::new())
            },
            &|| {},
        )
        .unwrap();
        drop(operation);
        let document = load().unwrap();
        assert_eq!(attempts, 3);
        assert_eq!(document.secrets[0].affected, ["dev"]);
        assert!(public(&document.secrets[0])["error"]
            .as_str()
            .unwrap()
            .contains("Retry"));
        use_test_store(None);
    }
    #[test]
    fn exhausted_reconcile_does_not_publish_success_for_a_newer_revision() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        use_test_store(Some(dir.path().join("secrets.json")));
        use_test_vault(Some(
            [
                ("private-reference".into(), "initial-value".into()),
                ("generation-1".into(), "rotation-1".into()),
                ("generation-2".into(), "rotation-2".into()),
                ("generation-3".into(), "rotation-3".into()),
            ]
            .into(),
        ));
        save(&Document {
            secrets: vec![secret()],
            ..Default::default()
        })
        .unwrap();
        let mut operation = Some(lock_unit(&OPERATION));
        let mut applied = Vec::new();
        reconcile_with(
            "id",
            &mut operation,
            &runtime_material,
            &mut |computer, material| {
                applied.push(material[0].1.clone());
                let _newer_save = lock_unit(&OPERATION);
                update(|document| {
                    document.secrets[0].value_id = format!("generation-{}", applied.len());
                    if applied.len() == 3 {
                        document.secrets[0]
                            .errors
                            .insert(computer.into(), "Newer update failed.".into());
                    }
                    Ok(())
                })?;
                Ok(Vec::new())
            },
            &|| {},
        )
        .unwrap();
        drop(operation);
        assert_eq!(applied, ["initial-value", "rotation-1", "rotation-2"]);
        let document = load().unwrap();
        let secret = &document.secrets[0];
        assert_eq!(secret.value_id, "generation-3");
        assert_eq!(secret.affected, ["dev"]);
        assert_eq!(
            secret.errors.get("dev").map(String::as_str),
            Some("Newer update failed.")
        );
        use_test_store(None);
        use_test_vault(None);
    }
    #[test]
    fn reconcile_releases_the_operation_lock_while_a_computer_applies() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        use_test_store(Some(dir.path().join("secrets.json")));
        save(&Document {
            secrets: vec![secret()],
            activities: Vec::new(),
            ..Default::default()
        })
        .unwrap();
        let mut operation = Some(lock_unit(&OPERATION));
        let mut applied = Vec::new();
        reconcile_with(
            "id",
            &mut operation,
            &|_| Ok(Vec::new()),
            &mut |computer, _| {
                // A fork or update check can take the lock while this computer applies.
                assert!(try_lock_unit(&OPERATION).is_some());
                applied.push(computer.to_string());
                if applied.len() == 1 {
                    // Another save changes this computer's desired secrets meanwhile.
                    update(|d| {
                        d.secrets[0].value_id = "rotated-reference".into();
                        Ok(())
                    })?;
                }
                Ok(Vec::new())
            },
            &|| {},
        )
        .unwrap();
        assert!(
            operation.is_some(),
            "the lock is held again when reconcile returns"
        );
        assert!(try_lock_unit(&OPERATION).is_none());
        drop(operation);
        assert_eq!(
            applied,
            ["dev", "dev"],
            "the newer desired state is applied again"
        );
        let document = load().unwrap();
        assert_eq!(public(&document.secrets[0])["state"], "active");
        use_test_store(None);
    }
    #[test]
    fn restart_finishing_during_apply_leaves_no_stale_restart_request() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        use_test_store(Some(dir.path().join("secrets.json")));
        let mut assigned = secret();
        assigned.computers = vec!["restarting".into()];
        assigned.affected = vec!["restarting".into()];
        save(&Document {
            secrets: vec![assigned],
            activities: Vec::new(),
            ..Default::default()
        })
        .unwrap();
        let mut operation = Some(lock_unit(&OPERATION));
        reconcile_with(
            "id",
            &mut operation,
            &|_| Ok(Vec::new()),
            &mut |computer, _| {
                // The running computer defers the change, but a restart completes with the
                // desired revision before this result is recorded.
                computer_started(computer, &computer_revision(computer)?)?;
                Ok(vec!["API_KEY".into()])
            },
            &|| {},
        )
        .unwrap();
        drop(operation);
        let document = load().unwrap();
        assert!(document.secrets[0].pending_computers.is_empty());
        assert_eq!(public(&document.secrets[0])["state"], "active");
        use_test_store(None);
    }
    #[test]
    fn removal_deletes_the_value_before_any_runtime_retry() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        use_test_store(Some(dir.path().join("secrets.json")));
        use_test_vault(Some(
            [("private-reference".into(), "private-value".into())].into(),
        ));
        save(&Document {
            secrets: vec![secret()],
            ..Default::default()
        })
        .unwrap();
        remove_from_store("id").unwrap();
        assert!(snapshot().unwrap().is_empty());
        assert!(read_vault().unwrap().is_empty());
        assert!(runtime_material("dev").unwrap().is_empty());
        assert_eq!(pending_names("dev").unwrap(), ["API_KEY"]);
        retry_revocations_with(&mut |_| Err("unreadable".into())).unwrap();
        assert_eq!(pending_names("dev").unwrap(), ["API_KEY"]);
        retry_revocations_with(&mut |_| Ok(true)).unwrap();
        assert!(pending_names("dev").unwrap().is_empty());
        use_test_store(None);
        use_test_vault(None);
    }

    #[test]
    fn removal_commits_while_an_older_secret_update_is_still_running() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        use_test_store(Some(dir.path().join("secrets.json")));
        use_test_vault(Some(
            [("private-reference".into(), "private-value".into())].into(),
        ));
        save(&Document {
            secrets: vec![secret()],
            ..Default::default()
        })
        .unwrap();
        let mut operation = Some(lock_unit(&OPERATION));
        reconcile_with(
            "id",
            &mut operation,
            &runtime_material,
            &mut |_, material| {
                assert_eq!(material[0].1, "private-value");
                let _remove = lock_unit(&OPERATION);
                remove_from_store("id")?;
                assert!(snapshot()?.is_empty());
                assert!(read_vault()?.is_empty());
                assert_eq!(pending_names("dev")?, ["API_KEY"]);
                Ok(Vec::new())
            },
            &|| {},
        )
        .unwrap();
        assert_eq!(
            pending_names("dev").unwrap(),
            ["API_KEY"],
            "the old update's completion must not clear this removal"
        );
        use_test_store(None);
        use_test_vault(None);
    }

    #[test]
    fn verified_replacement_clears_old_records_without_clearing_a_later_generation() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        use_test_store(Some(dir.path().join("secrets.json")));
        use_test_vault(Some(
            [("private-reference".into(), "private-value".into())].into(),
        ));
        save(&Document {
            secrets: vec![secret()],
            ..Default::default()
        })
        .unwrap();
        remove_from_store("id").unwrap();
        let before_apply = pending_revocations().unwrap();
        revocations_replaced(&before_apply, "dev", &["API_KEY".into()]).unwrap();
        assert_eq!(
            pending_names("dev").unwrap(),
            ["API_KEY"],
            "a next-start addition has not replaced the old live value"
        );
        let mut replacement = secret();
        replacement.id = "replacement".into();
        replacement.value_id = "new-value".into();
        update(|document| {
            document.secrets.push(replacement);
            Ok(())
        })
        .unwrap();
        remove_from_store("replacement").unwrap();
        revocations_replaced(&before_apply, "dev", &[]).unwrap();
        let remaining = pending_revocations().unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].generation, "new-value");
        use_test_store(None);
        use_test_vault(None);
    }

    #[test]
    fn restart_and_deletion_clear_only_their_computer_revocations() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        use_test_store(Some(dir.path().join("secrets.json")));
        use_test_vault(Some(
            [("private-reference".into(), "private-value".into())].into(),
        ));
        let mut assigned = secret();
        assigned.computers.push("other".into());
        save(&Document {
            secrets: vec![assigned],
            ..Default::default()
        })
        .unwrap();
        let before = computer_revision("dev").unwrap();
        remove_from_store("id").unwrap();
        computer_started("dev", &before).unwrap();
        assert_eq!(
            pending_names("dev").unwrap(),
            ["API_KEY"],
            "an obsolete boot still had the value"
        );
        computer_started("dev", &computer_revision("dev").unwrap()).unwrap();
        assert!(pending_names("dev").unwrap().is_empty());
        assert_eq!(pending_names("other").unwrap(), ["API_KEY"]);
        computer_removed("other").unwrap();
        assert!(load().unwrap().pending_revocations.is_empty());
        use_test_store(None);
        use_test_vault(None);
    }

    #[test]
    fn readding_the_name_never_lets_an_old_retry_remove_the_replacement() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        use_test_store(Some(dir.path().join("secrets.json")));
        use_test_vault(Some(
            [("private-reference".into(), "private-value".into())].into(),
        ));
        save(&Document {
            secrets: vec![secret()],
            ..Default::default()
        })
        .unwrap();
        remove_from_store("id").unwrap();
        let old = load().unwrap().pending_revocations[0].clone();
        assert!(revocation_needed(&old).unwrap());
        assert!(validate(&request(), &load().unwrap()).is_ok());
        let mut replacement = secret();
        replacement.id = "replacement".into();
        replacement.value_id = "new-generation".into();
        update(|d| {
            d.secrets.push(replacement);
            Ok(())
        })
        .unwrap();
        assert!(
            !revocation_needed(&old).unwrap(),
            "the computer gate rechecks this immediately before name-only removal"
        );
        // A delayed success for the old generation cannot clear a later removal.
        write_vault([("new-generation".into(), "new-value".into())].into()).unwrap();
        remove_from_store("replacement").unwrap();
        retry_revocations_with(&mut |record| Ok(record.generation == old.generation)).unwrap();
        let pending = load().unwrap().pending_revocations;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].generation, "new-generation");
        use_test_store(None);
        use_test_vault(None);
    }

    #[test]
    fn history_is_bounded_and_contains_no_values() {
        let _test_state = crate::test_support::global_state();
        let mut d = Document::default();
        for _ in 0..110 {
            event(&mut d, "Secret settings saved", false);
        }
        assert_eq!(d.activities.len(), 100);
        assert!(d
            .activities
            .iter()
            .all(|e| e["category"] == "secrets" && e["status"] == "completed"));
        assert!(!serde_json::to_string(&d).unwrap().contains("private-value"));
    }
}

pub(crate) fn update_guard() -> Result<std::sync::MutexGuard<'static, ()>, String> {
    try_lock_unit(&OPERATION)
        .ok_or_else(|| "Wait for the secret operation to finish before updating.".into())
}
