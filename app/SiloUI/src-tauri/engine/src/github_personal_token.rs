//! Personal tokens are secrets of kind `github-token` (see `secrets.rs`). A computer's
//! GitHub access names one of them; only host runtime profiles receive the value.
use super::*;
use std::collections::{BTreeMap, HashMap};

/// The name the single-token methods of earlier versions use for the secret they manage.
const LEGACY_NAME: &str = "github-token";

/// A token saved by earlier versions: one credential-store entry for the whole app.
#[derive(Deserialize)]
struct LegacyToken {
    token: String,
    account: String,
}
static TOKEN_OPERATION: Mutex<()> = Mutex::new(());
/// What the last check learned about each token secret, by secret id.
static STATUS: Mutex<BTreeMap<String, Status>> = Mutex::new(BTreeMap::new());
/// Set once a client has used `github.personalToken.save` or `.remove` in this session.
static LEGACY_CLIENT: AtomicBool = AtomicBool::new(false);
static CHECK_AT: AtomicU64 = AtomicU64::new(0);
static APPLIED: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

#[derive(Clone, PartialEq, Eq, Debug)]
struct Status {
    connected: bool,
    message: Option<String>,
}
impl Status {
    fn connected() -> Self {
        Status {
            connected: true,
            message: None,
        }
    }
    fn disconnected(message: impl Into<String>) -> Self {
        Status {
            connected: false,
            message: Some(message.into()),
        }
    }
}

pub(super) fn selected(policy: &Value) -> bool {
    policy["authenticationMethod"] == "token"
}
/// The token secret a computer's policy chooses, when it uses a token at all.
pub(super) fn chosen(policy: &Value) -> Option<&str> {
    selected(policy)
        .then(|| policy["tokenSecretId"].as_str())
        .flatten()
}
fn statuses() -> MutexGuard<'static, BTreeMap<String, Status>> {
    STATUS.lock().unwrap_or_else(PoisonError::into_inner)
}
/// The public connection state of one token secret.
pub(crate) fn connection(id: &str, removing: bool) -> Value {
    if removing {
        return json!({"state":"disconnected","message":"Removal is pending. Retry removing this token."});
    }
    match statuses().get(id) {
        Some(Status {
            connected: true, ..
        }) => json!({"state":"connected"}),
        Some(Status {
            message: Some(message),
            ..
        }) => json!({"state":"disconnected","message":message}),
        Some(_) => json!({"state":"disconnected"}),
        None => {
            json!({"state":"disconnected","message":"Waiting for GitHub to confirm this token."})
        }
    }
}
/// The state of the secret earlier versions called "the personal token".
pub(super) fn status() -> Value {
    let legacy = crate::secrets::github_token_entries()
        .ok()
        .and_then(|entries| {
            entries
                .into_iter()
                .find(|entry| entry.name == LEGACY_NAME && !entry.removing)
        });
    let Some(entry) = legacy else {
        return json!({"state":"disconnected","saved":false});
    };
    let connection = connection(&entry.id, false);
    let mut status = json!({"state":connection["state"],"saved":true});
    if let Some(account) = entry.account {
        status["account"] = json!(account);
    }
    if let Some(message) = connection.get("message") {
        status["message"] = message.clone();
    }
    status
}
/// The policies as the app reads them: a computer whose chosen token secret no longer exists
/// is marked `tokenMissing`. It keeps failing closed; the mark only says why.
pub(super) fn with_missing_marks(computers: &[Value]) -> Vec<Value> {
    let Ok(entries) = crate::secrets::github_token_entries() else {
        return computers.to_vec();
    };
    computers
        .iter()
        .map(|policy| {
            let mut policy = policy.clone();
            if let Some(id) = chosen(&policy) {
                if !entries.iter().any(|entry| entry.id == id) {
                    policy["tokenMissing"] = json!(true);
                }
            }
            policy
        })
        .collect()
}
pub(super) fn connected(id: &str) -> bool {
    statuses().get(id).is_some_and(|status| status.connected)
}
/// The computers whose GitHub access uses each token secret.
pub(crate) fn users() -> BTreeMap<String, Vec<String>> {
    let mut users: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for policy in load().map(|d| d.computers).unwrap_or_default() {
        if let (Some(id), Some(computer)) = (chosen(&policy), policy["computer"].as_str()) {
            users
                .entry(id.to_owned())
                .or_default()
                .push(computer.to_owned());
        }
    }
    users
}
fn user_lookup(token: &str) -> Result<Value, String> {
    #[cfg(test)]
    if let Some(result) = TEST_LOOKUP.with(|lookup| lookup.borrow().as_ref().map(|f| f(token))) {
        return result;
    }
    github(token, "/user")
}
#[cfg(test)]
type TestLookup = Box<dyn Fn(&str) -> Result<Value, String>>;
#[cfg(test)]
thread_local! {
    static TEST_LOOKUP: std::cell::RefCell<Option<TestLookup>> = const { std::cell::RefCell::new(None) };
}
/// Tests on this thread answer GitHub's `/user` with `lookup` instead of the network.
#[cfg(test)]
pub(crate) fn use_test_lookup(lookup: Option<TestLookup>) {
    TEST_LOOKUP.with(|test| *test.borrow_mut() = lookup);
}
/// Asks GitHub whose token this is. Returns the account; never the token or GitHub's reply.
pub(crate) fn validate_token(token: &str) -> Result<String, String> {
    let _network = serialize(&TOKEN_OPERATION);
    // Saving a token is an explicit retry; never leave validation blocked by earlier failures.
    crate::github_http::reset_bearer_retries(token);
    validated_with(token, user_lookup)
}
/// What GitHub said about a token.
#[derive(Debug, PartialEq, Eq)]
enum Validation {
    /// GitHub accepted the token; this is its account.
    Accepted(String),
    /// GitHub answered that the token is not valid.
    Rejected(String),
    /// GitHub could not be asked, or its answer says nothing about the token: a timeout, a
    /// server error, a rate limit, no network.
    Unknown,
}
const NOT_ACCEPTED: &str =
    "GitHub did not accept this token. Check that it is valid and has not expired.";
const NOT_REACHED: &str = "Silo could not reach GitHub to check this token. Try again.";
fn validation(token: &str, lookup: impl FnOnce(&str) -> Result<Value, String>) -> Validation {
    if token.is_empty()
        || token.len() > 4096
        || !token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Validation::Rejected("Enter a valid GitHub personal access token.".into());
    }
    match lookup(token) {
        Err(error) if crate::github_http::credentials_rejected(&error) => {
            Validation::Rejected(NOT_ACCEPTED.into())
        }
        Err(_) => Validation::Unknown,
        Ok(user) => user["login"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 100)
            .map_or(Validation::Unknown, |login| {
                Validation::Accepted(login.to_owned())
            }),
    }
}
/// The account of a token GitHub accepts; an error for the person otherwise.
fn validated_with(
    token: &str,
    lookup: impl FnOnce(&str) -> Result<Value, String>,
) -> Result<String, String> {
    match validation(token, lookup) {
        Validation::Accepted(account) => Ok(account),
        Validation::Rejected(message) => Err(message),
        Validation::Unknown => Err(NOT_REACHED.into()),
    }
}
/// The value of one connected token secret. A disconnected one is never handed out.
pub(super) fn value_for(id: &str) -> Result<String, String> {
    if !connected(id) {
        return Err(
            "The chosen GitHub token is disconnected. Replace it in Secrets or choose another."
                .into(),
        );
    }
    crate::secrets::github_token_value(id)
}
/// The value of the token a computer's policy chooses.
pub(super) fn value_of(policy: &Value) -> Result<String, String> {
    value_for(chosen(policy).ok_or("Choose a GitHub token for this computer.")?)
}
/// The profile the proxy receives for a token: the same version-2 shape as always.
pub(super) fn token_profile(token: &str) -> Value {
    json!({"version":2,"owners":[],"personalToken":token})
}
fn fingerprint(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}
/// The fingerprints of connected tokens from memory only, like `value_for` without the store.
fn current_fingerprints() -> HashMap<String, String> {
    let connected: Vec<String> = statuses()
        .iter()
        .filter(|(_, status)| status.connected)
        .map(|(id, _)| id.clone())
        .collect();
    if connected.is_empty() {
        return HashMap::new();
    }
    crate::secrets::peek_github_token_values()
        .into_iter()
        .filter(|(id, _)| connected.contains(id))
        .map(|(id, value)| (id, fingerprint(&value)))
        .collect()
}
fn applied() -> &'static Mutex<HashMap<String, String>> {
    APPLIED.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn apply(name: &str, secret: Option<&str>, revision: u64) -> Result<(), String> {
    // The credential store can wait on a permission prompt; never read it under STATE.
    let token = secret
        .ok_or_else(|| "Choose a GitHub token for this computer.".to_owned())
        .and_then(value_for);
    let key = active_key(name)?;
    outside_state(
        || {
            let d = load()?;
            if d.revision != revision {
                return Err("GitHub access changed. Applying your latest choices.".into());
            }
            // Narrowing has already detached the token; never attach it while access is disabled.
            if !d.access_enabled {
                return Ok(None);
            }
            let token = token?;
            let profile = token_profile(&token);
            let mut applied = applied()
                .lock()
                .map_err(|_| "GitHub token state is unavailable.")?;
            if runtime()?.github_policy_is_cached(name, &profile)?
                && applied.get(&key) == Some(&fingerprint(&token))
            {
                return Ok(None);
            }
            // Record the attachment before `msb modify` runs without STATE, so a narrowing
            // meanwhile (a switch, removal or Disable access) detaches the token after it.
            applied.insert(key.clone(), fingerprint(&token));
            active()
                .lock()
                .map_err(|_| "GitHub state is unavailable.")?
                .remove(&key);
            Ok(Some(profile))
        },
        |profile| runtime()?.apply_github_policy(name, revision, &profile),
    )?
    .unwrap_or(Ok(()))
}

/// Disable access is a global kill switch: no computer keeps a token while it is off.
/// A computer keeps its token only while the token it chose is still the one attached.
fn keeps_token(
    d: &Document,
    name: &str,
    current: &HashMap<String, String>,
    attached: &String,
) -> bool {
    d.access_enabled
        && d.computers
            .iter()
            .find(|w| w["computer"] == name)
            .and_then(chosen)
            .is_some_and(|id| current.get(id) == Some(attached))
}
/// Remove token authority before a method switch, removal, failed validation or replacement.
/// OAuth reconciliation never substitutes its own credential for a disconnected personal token.
/// Every computer is detached even when another fails; failures are reported per computer.
pub(super) fn narrow(d: &Document, errors: &mut NarrowErrors) {
    let prefix = match path() {
        Ok(path) => format!("{}:", path.display()),
        Err(error) => return errors.record_all(error),
    };
    let cached = match applied().lock() {
        Ok(cached) => cached.clone(),
        Err(_) => return errors.record_all("GitHub token state is unavailable.".into()),
    };
    // Narrowing runs under STATE, so it never opens the credential store (which can wait
    // on a permission prompt). A token not read yet this session was never attached.
    let current = current_fingerprints();
    let detach = |name: &str, key: &str| -> Result<(), String> {
        detach_result(
            name,
            runtime()?.apply_github_policy(name, d.revision, &profile(&[])),
        )?;
        applied()
            .lock()
            .map_err(|_| "GitHub token state is unavailable.")?
            .remove(key);
        Ok(())
    };
    let stale: Vec<(String, String)> = cached
        .iter()
        .filter(|(key, _)| key.starts_with(&prefix))
        .filter_map(|(key, attached)| {
            let name = &key[prefix.len()..];
            (!keeps_token(d, name, &current, attached)).then(|| (name.to_owned(), key.clone()))
        })
        .collect();
    each_computer(
        stale
            .iter()
            .map(|(name, key)| (name.as_str(), key.as_str())),
        errors,
        |name, key| detach(name, key),
    );
    // A surviving runtime may still hold a token from the preceding app process.
    if d.session != session() {
        let mut restored = Vec::new();
        for w in d.computers.iter().filter(|w| selected(w)) {
            let Some(name) = w["computer"].as_str() else {
                errors.record_all("Invalid computer policy.".into());
                continue;
            };
            match active_key(name) {
                Ok(key) => restored.push((name.to_owned(), key)),
                Err(error) => errors.record(name, error),
            }
        }
        each_computer(
            restored
                .iter()
                .map(|(name, key)| (name.as_str(), key.as_str())),
            errors,
            |name, key| {
                applied()
                    .lock()
                    .map_err(|_| "GitHub token state is unavailable.")?
                    .entry(key.to_owned())
                    .or_insert_with(|| "unverified".into());
                detach(name, key)
            },
        );
    }
}
/// The computers whose access follows `secrets` (or every token computer for `None`).
fn computers_using(d: &Document, secrets: Option<&[&str]>) -> Vec<String> {
    d.computers
        .iter()
        .filter(|w| selected(w))
        .filter(|w| match secrets {
            None => true,
            Some(ids) => chosen(w).is_some_and(|id| ids.contains(&id)),
        })
        .filter_map(|w| w["computer"].as_str().map(str::to_owned))
        .collect()
}
fn changed(secrets: Option<&[&str]>) -> Result<(), String> {
    let _state = serialize(&STATE);
    let mut d = load()?;
    d.revision = next_policy_revision(d.revision)?;
    let names = computers_using(&d, secrets);
    for name in &names {
        if !d.access_pending.contains(name) {
            d.access_pending.push(name.clone());
        }
    }
    mark_pending_for(&mut d, &names);
    save(&d)?;
    let mut errors = NarrowErrors::default();
    narrow(&d, &mut errors);
    let result = errors.into_result();
    schedule(Duration::ZERO);
    publish_changed();
    result
}

/// A deleted computer no longer holds the token; forget its attachment.
pub(super) fn forget(key: &str) {
    applied()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(key);
}
/// Time until `check` validates the tokens again, so the worker can sleep until then.
pub(super) fn next_check() -> Duration {
    Duration::from_secs(CHECK_AT.load(Ordering::SeqCst).saturating_sub(now()))
}

/// What a check concluded about one token secret.
#[derive(PartialEq, Eq, Debug)]
struct Verdict {
    id: String,
    /// The stored value this verdict is about; a replaced token no longer matches it.
    value_id: String,
    /// Whether the secret was being removed when the verdict was made.
    removing: bool,
    /// `None` when GitHub could not be asked: the previous status stands.
    status: Option<Status>,
    account: Option<String>,
}
/// Validates every token secret. `values` is read only when some token is live, and
/// `lookup` answers GitHub's `/user`. A failure of one token never changes another's verdict.
fn evaluate(
    entries: &[crate::secrets::TokenEntry],
    values: impl FnOnce() -> Result<BTreeMap<String, crate::secrets::TokenValue>, String>,
    lookup: impl Fn(&str) -> Result<Value, String>,
) -> Vec<Verdict> {
    let live = entries.iter().any(|entry| !entry.removing);
    let values = if live { Some(values()) } else { None };
    entries
        .iter()
        .map(|entry| {
            let mut value_id = entry.value_id.clone();
            let (status, account) = if entry.removing {
                (
                    Some(Status::disconnected(
                        "Removal is pending. Retry removing this token.",
                    )),
                    None,
                )
            } else {
                match values.as_ref() {
                    Some(Ok(values)) => match values.get(&entry.id) {
                        Some(token) => {
                            value_id = token.value_id.clone();
                            match validation(&token.value, &lookup) {
                                Validation::Accepted(account) => {
                                    (Some(Status::connected()), Some(account))
                                }
                                Validation::Rejected(message) => {
                                    (Some(Status::disconnected(message)), None)
                                }
                                Validation::Unknown => (None, None),
                            }
                        }
                        None => (
                            Some(Status::disconnected(
                                "The token's value is missing. Replace it.",
                            )),
                            None,
                        ),
                    },
                    Some(Err(message)) => (Some(Status::disconnected(message.clone())), None),
                    None => (
                        Some(Status::disconnected("The token could not be read.")),
                        None,
                    ),
                }
            };
            Verdict {
                id: entry.id.clone(),
                value_id,
                removing: entry.removing,
                status,
                account,
            }
        })
        .collect()
}
/// Whether a verdict still describes the secret as it is now: the same stored value and
/// the same removal state.
fn describes(verdict: &Verdict, current: &[crate::secrets::TokenEntry]) -> bool {
    current.iter().any(|entry| {
        entry.id == verdict.id
            && entry.value_id == verdict.value_id
            && entry.removing == verdict.removing
    })
}
/// Records verdicts and returns the ids whose status changed. A secret in `live` that the
/// verdicts do not cover keeps its status; one no longer in `live` is forgotten.
fn publish_verdicts(verdicts: &[Verdict], live: &[&str]) -> Vec<String> {
    let mut statuses = statuses();
    let mut changed: Vec<String> = statuses
        .keys()
        .filter(|id| !live.contains(&id.as_str()))
        .cloned()
        .collect();
    for id in &changed {
        statuses.remove(id);
    }
    for verdict in verdicts {
        let Some(status) = &verdict.status else {
            continue;
        };
        if statuses.get(&verdict.id) != Some(status) {
            statuses.insert(verdict.id.clone(), status.clone());
            changed.push(verdict.id.clone());
        }
    }
    changed
}
/// Called by the existing serialized worker. Snapshot reads never touch secure storage.
pub(super) fn check() {
    if now() < CHECK_AT.load(Ordering::SeqCst) {
        return;
    }
    CHECK_AT.store(now() + 300, Ordering::SeqCst);
    let Ok(entries) = crate::secrets::github_token_entries() else {
        CHECK_AT.store(now() + 30, Ordering::SeqCst);
        return;
    };
    // GitHub is asked without holding TOKEN_OPERATION, so a save never waits for the lookups.
    let verdicts = evaluate(&entries, crate::secrets::github_token_values, user_lookup);
    if verdicts
        .iter()
        .any(|v| v.status.as_ref().is_none_or(|s| !s.connected) && !v.removing)
    {
        CHECK_AT.store(now() + 30, Ordering::SeqCst);
    }
    let (account_changed, changed_ids) = {
        let _operation = serialize(&TOKEN_OPERATION);
        // A save, replacement or removal may have happened during the lookups; only
        // verdicts about the secret as it is now are recorded.
        let Ok(current) = crate::secrets::github_token_entries() else {
            CHECK_AT.store(now() + 30, Ordering::SeqCst);
            return;
        };
        let verdicts: Vec<Verdict> = verdicts
            .into_iter()
            .filter(|verdict| describes(verdict, &current))
            .collect();
        let mut account_changed = false;
        for verdict in &verdicts {
            if let Some(account) = &verdict.account {
                account_changed |= crate::secrets::set_github_token_account(
                    &verdict.id,
                    &verdict.value_id,
                    account,
                )
                .unwrap_or(false);
            }
        }
        let live: Vec<&str> = current.iter().map(|entry| entry.id.as_str()).collect();
        (account_changed, publish_verdicts(&verdicts, &live))
    };
    if account_changed || !changed_ids.is_empty() {
        crate::secrets::publish_changed();
    }
    if changed_ids.is_empty() {
        return;
    }
    let ids: Vec<&str> = changed_ids.iter().map(String::as_str).collect();
    if let Err(message) = changed(Some(&ids)) {
        // Keep the failed detachment visible and retry it through the normal worker.
        let _state = serialize(&STATE);
        if let Ok(mut d) = load() {
            for name in computers_using(&d, Some(&ids)) {
                d.access_errors.insert(name, message.clone());
            }
            let _ = save(&d);
        }
    }
}

/// Sets what is known about a token secret, or forgets it for `None`. Serialized with the
/// check, so a verdict being recorded never overwrites a newer state.
fn set_status(id: &str, status: Option<Status>) {
    let _operation = serialize(&TOKEN_OPERATION);
    match status {
        Some(status) => statuses().insert(id.to_owned(), status),
        None => statuses().remove(id),
    };
}

/// A token secret was saved with a new value that GitHub accepted: its computers pick it up.
pub(crate) fn secret_saved(id: &str) -> Result<(), String> {
    set_status(id, Some(Status::connected()));
    CHECK_AT.store(now() + 300, Ordering::SeqCst);
    changed(Some(&[id]))
}
/// The user asked to try a token again.
pub(crate) fn secret_retry(id: &str) -> Result<(), String> {
    CHECK_AT.store(0, Ordering::SeqCst);
    set_status(id, None);
    changed(Some(&[id]))
}
/// A token secret is being removed: detach every computer using it before its value goes.
pub(crate) fn secret_removing(id: &str) -> Result<(), String> {
    set_status(id, Some(Status::disconnected("Removing this token.")));
    changed(Some(&[id]))
}
/// A token secret and its value are gone.
pub(crate) fn secret_removed(id: &str) {
    set_status(id, None);
    let _ = changed(Some(&[id]));
}

/// A save from a client that knows only "token mode" names no token: it means the secret
/// earlier versions managed, when there is one. This applies only after such a client has
/// used the single-token methods; any other save that selects token mode must name a token.
pub(super) fn with_legacy_choice(mut configuration: Value) -> Value {
    if !LEGACY_CLIENT.load(Ordering::SeqCst) {
        return configuration;
    }
    let Some(policies) = configuration["computers"].as_array_mut() else {
        return configuration;
    };
    if !policies
        .iter()
        .any(|w| selected(w) && w["tokenSecretId"].is_null())
    {
        return configuration;
    }
    if let Ok(Some(id)) = crate::secrets::github_token_named(LEGACY_NAME) {
        for policy in policies
            .iter_mut()
            .filter(|w| selected(w) && w["tokenSecretId"].is_null())
        {
            policy["tokenSecretId"] = json!(id);
        }
    }
    configuration
}

/// Earlier versions' `github.personalToken.save`: saves the token as the secret named
/// `github-token`. Computers using it keep using it.
pub fn save_github_personal_token(token: String) -> Result<Value, String> {
    LEGACY_CLIENT.store(true, Ordering::SeqCst);
    let existing = crate::secrets::github_token_named(LEGACY_NAME)?;
    crate::secrets::save_secret(
        crate::secrets::github_token_request(existing, LEGACY_NAME, Some(token)),
        || Ok(()),
    )?;
    snapshot()
}

/// Earlier versions' `github.personalToken.remove`: removes the secret named `github-token`.
pub fn remove_github_personal_token() -> Result<Value, String> {
    LEGACY_CLIENT.store(true, Ordering::SeqCst);
    if let Some(id) = crate::secrets::github_token_named(LEGACY_NAME)? {
        crate::secrets::remove_secret(id, || Ok(()))?;
    }
    snapshot()
}

/// Points every token computer that names no token at `id`.
fn repoint(d: &mut Document, id: &str) -> bool {
    let mut names = Vec::new();
    for policy in d.computers.iter_mut().filter(|w| selected(w)) {
        if policy["tokenSecretId"].is_null() {
            policy["tokenSecretId"] = json!(id);
            if let Some(name) = policy["computer"].as_str() {
                names.push(name.to_owned());
            }
        }
    }
    if names.is_empty() {
        return false;
    }
    if let Ok(revision) = next_policy_revision(d.revision) {
        d.revision = revision;
    }
    for name in names {
        stamp(d, &name, None);
    }
    true
}

/// The parts of the move to token secrets, so each can be interrupted in a test.
struct Migration<'a> {
    removal_pending: &'a dyn Fn() -> Result<bool, String>,
    clear_removal: &'a dyn Fn() -> Result<(), String>,
    read_legacy: &'a dyn Fn() -> Result<Option<LegacyToken>, String>,
    import: &'a dyn Fn(&LegacyToken) -> Result<String, String>,
    repoint: &'a dyn Fn(&str) -> Result<(), String>,
    delete_legacy: &'a dyn Fn() -> Result<(), String>,
}
/// Moves the saved single token into a token secret. Each step can be repeated: the
/// import reuses a secret that already holds the token, re-pointing only touches computers
/// that name no token yet, and the old entry goes last, so an interruption anywhere leaves
/// the next start to finish.
fn migrate_with(m: &Migration) -> Result<(), String> {
    if (m.removal_pending)()? {
        // The token was being removed: finish that rather than moving it.
        (m.delete_legacy)()?;
        return (m.clear_removal)();
    }
    let Some(legacy) = (m.read_legacy)()? else {
        return Ok(());
    };
    let id = (m.import)(&legacy)?;
    (m.repoint)(&id)?;
    (m.delete_legacy)()
}
fn legacy_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(
        crate::channel::current().keychain_service(crate::channel::Keychain::Github),
        "personal-token",
    )
    .map_err(|_| "The system credential store is unavailable.".into())
}
/// A failed move is logged once and listed in the Secrets activity; the next start tries again.
pub(super) fn migration_failed(message: &str) {
    eprintln!("Could not move the saved GitHub token into Secrets: {message}");
    crate::secrets::record_failure(&format!(
        "Could not move the saved GitHub token into Secrets: {message} Silo tries again at the next start."
    ));
}
/// Run once at engine start, before the worker: a token saved by an earlier version
/// becomes the secret `github-token` (or `github-token-2`, and so on, when that name is
/// taken) and the computers that used it point at it.
pub(super) fn migrate_legacy() -> Result<(), String> {
    crate::secrets::init_path();
    migrate_with(&Migration {
        removal_pending: &|| {
            let _state = serialize(&STATE);
            Ok(load()?.personal_token_removing)
        },
        clear_removal: &|| {
            let _state = serialize(&STATE);
            let mut d = load()?;
            d.personal_token_removing = false;
            save(&d)
        },
        read_legacy: &|| match legacy_entry()?.get_password() {
            Ok(value) => serde_json::from_str(&value)
                .map(Some)
                .map_err(|_| "Stored GitHub token is invalid.".into()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(crate::credential_store::describe(
                &error,
                "Cannot read the GitHub token from the system credential store.",
            )),
        },
        import: &|legacy| {
            crate::secrets::import_github_token(&legacy.token, &legacy.account, LEGACY_NAME)
        },
        repoint: &|id| {
            let _state = serialize(&STATE);
            let mut d = load()?;
            if repoint(&mut d, id) {
                save(&d)?;
            }
            Ok(())
        },
        delete_legacy: &|| match legacy_entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(crate::credential_store::describe(
                &error,
                "Cannot remove the old GitHub token from the system credential store.",
            )),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    /// A disposable secrets document and vault, with no token statuses, for one test.
    struct Fixture {
        _global: crate::test_support::GlobalStateGuard,
        _directory: tempfile::TempDir,
    }
    impl Fixture {
        fn new() -> Self {
            let global = crate::test_support::global_state();
            let directory = tempfile::tempdir().unwrap();
            crate::secrets::use_test_store(Some(directory.path().join("secrets.json")));
            crate::secrets::use_test_vault(Some(Default::default()));
            statuses().clear();
            Fixture {
                _global: global,
                _directory: directory,
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            crate::secrets::use_test_store(None);
            crate::secrets::use_test_vault(None);
            use_test_lookup(None);
            LEGACY_CLIENT.store(false, Ordering::SeqCst);
            statuses().clear();
        }
    }
    fn token_policy(computer: &str, secret: Option<&str>) -> Value {
        json!({"computer":computer,"authenticationMethod":"token","tokenSecretId":secret})
    }
    fn connect(ids: &[&str]) {
        for id in ids {
            statuses().insert((*id).into(), Status::connected());
        }
    }

    #[test]
    fn empty_account_connects_without_repository_discovery() {
        let account = validated_with("github_pat_synthetic", |_| {
            Ok(json!({"login":"new-user","public_repos":0}))
        })
        .unwrap();
        assert_eq!(account, "new-user");
    }
    #[test]
    fn validation_does_not_echo_tokens_or_upstream_errors() {
        let error = validated_with("github_pat_synthetic", |_| {
            Err("github_pat_synthetic private response".into())
        })
        .err()
        .unwrap();
        assert!(!error.contains("github_pat_synthetic"));
        assert!(!error.contains("private response"));
        assert!(
            validated_with("a\r\nAuthorization: bad", |_| panic!("invalid token sent")).is_err()
        );
    }
    #[test]
    fn choosing_a_token_needs_a_connected_one() {
        let connected = |id: &str| id == "good";
        let oauth = json!({"authenticationMethod":"oauth"});
        let good = token_policy("dev", Some("good"));
        let bad = token_policy("dev", Some("bad"));
        let none = token_policy("dev", None);
        // Switching to tokens, or to another token, needs a connected token.
        // A save that selects token access without naming a token says what is missing.
        let missing = validate_method_change(None, &none, true, &connected).unwrap_err();
        assert!(
            missing.contains("Choose a connected GitHub token"),
            "{missing}"
        );
        assert!(validate_method_change(None, &bad, true, &connected).is_err());
        assert!(validate_method_change(Some(&oauth), &bad, true, &connected).is_err());
        assert!(validate_method_change(Some(&oauth), &good, false, &connected).is_ok());
        assert!(validate_method_change(Some(&good), &bad, true, &connected).is_err());
        // A choice that is already made survives unrelated edits, connected or not.
        assert!(validate_method_change(Some(&bad), &bad, true, &connected).is_ok());
        assert!(validate_method_change(Some(&none), &none, true, &connected).is_ok());
        // Going back to sign-in needs the sign-in.
        assert!(validate_method_change(Some(&good), &oauth, false, &connected).is_err());
        assert!(validate_method_change(Some(&good), &oauth, true, &connected).is_ok());
        assert!(validate_method_change(None, &oauth, false, &connected).is_ok());
    }
    #[test]
    fn a_token_is_only_named_with_token_access() {
        let mut policy = json!({
            "computer":"dev","identity":{"name":"","email":"","apply":false},
            "repositoryMode":"selected","repositories":[],"allRepositoriesAllowChanges":false,
            "authenticationMethod":"token","tokenSecretId":"id"
        });
        assert!(validate(std::slice::from_ref(&policy)).is_ok());
        policy["tokenSecretId"] = Value::Null;
        assert!(validate(std::slice::from_ref(&policy)).is_ok());
        policy["tokenSecretId"] = json!("id");
        policy["authenticationMethod"] = json!("oauth");
        assert!(validate(std::slice::from_ref(&policy)).is_err());
        policy["authenticationMethod"] = json!("token");
        policy["tokenSecretId"] = json!(7);
        assert!(validate(std::slice::from_ref(&policy)).is_err());
    }
    #[test]
    fn a_changed_token_choice_is_an_access_change() {
        let a = token_policy("dev", Some("a"));
        let b = token_policy("dev", Some("b"));
        assert_ne!(access_choice(&a), access_choice(&b));
        assert_eq!(access_choice(&a), access_choice(&a.clone()));
    }
    #[test]
    fn disable_access_detaches_personal_token_computers() {
        let attached = "attached".to_string();
        let current = HashMap::from([("a".to_string(), attached.clone())]);
        let mut d = Document {
            access_enabled: true,
            computers: vec![
                token_policy("dev", Some("a")),
                token_policy("other", Some("b")),
            ],
            ..Document::default()
        };
        assert!(keeps_token(&d, "dev", &current, &attached));
        // The token a computer chose is not the one attached, or is not connected.
        assert!(!keeps_token(&d, "other", &current, &attached));
        assert!(!keeps_token(&d, "dev", &HashMap::new(), &attached));
        assert!(!keeps_token(&d, "dev", &current, &"older".to_string()));
        d.access_enabled = false;
        assert!(!keeps_token(&d, "dev", &current, &attached));
    }
    #[test]
    fn legacy_policies_stay_oauth_and_token_policy_has_no_oauth_scopes() {
        assert!(!selected(&json!({})));
        assert!(selected(&json!({"authenticationMethod":"token"})));
        assert_eq!(chosen(&json!({"tokenSecretId":"id"})), None);
        assert_eq!(chosen(&token_policy("dev", Some("id"))), Some("id"));
        let d = Document {
            access_enabled: true,
            repositories: vec![json!({"id":1,"ownerId":1,"name":"owner/repo"})],
            ..Document::default()
        };
        assert!(scopes(
            &d,
            &json!({"authenticationMethod":"token","repositoryMode":"all","repositories":[]})
        )
        .unwrap()
        .is_empty());
    }

    #[test]
    fn each_computer_gets_the_profile_of_its_own_token() {
        let _fixture = Fixture::new();
        let a = crate::secrets::import_github_token("synthetic_token_a", "alice", "work").unwrap();
        let b = crate::secrets::import_github_token("synthetic_token_b", "bob", "home").unwrap();
        connect(&[&a, &b]);
        let profile_of = |policy: &Value| token_profile(&value_of(policy).unwrap());
        assert_eq!(
            profile_of(&token_policy("one", Some(&a))),
            json!({"version":2,"owners":[],"personalToken":"synthetic_token_a"})
        );
        assert_eq!(
            profile_of(&token_policy("two", Some(&b))),
            json!({"version":2,"owners":[],"personalToken":"synthetic_token_b"})
        );
        // No choice, or a token that no longer exists, gives no profile.
        assert!(value_of(&token_policy("three", None)).is_err());
        statuses().insert("gone".into(), Status::connected());
        assert!(value_of(&token_policy("three", Some("gone"))).is_err());
    }
    #[test]
    fn host_push_uses_the_chosen_token_and_never_falls_back() {
        let _fixture = Fixture::new();
        let a = crate::secrets::import_github_token("synthetic_token_a", "alice", "work").unwrap();
        let b = crate::secrets::import_github_token("synthetic_token_b", "bob", "home").unwrap();
        connect(&[&a, &b]);
        let push = |secret: &str| {
            personal_push_credential(&token_policy("dev", Some(secret)), "owner/repo")
        };
        assert_eq!(push(&a).unwrap().token, "synthetic_token_a");
        assert_eq!(push(&b).unwrap().token, "synthetic_token_b");
        // The chosen token is disconnected: the push fails instead of using the other one.
        statuses().insert(a.clone(), Status::disconnected("rejected"));
        assert!(push(&a).is_err());
        assert_eq!(push(&b).unwrap().token, "synthetic_token_b");
    }
    #[test]
    fn a_token_secret_is_never_an_environment_placeholder() {
        let _fixture = Fixture::new();
        let id = crate::secrets::import_github_token("synthetic_token_a", "alice", "work").unwrap();
        // Even a document edited by hand to assign the token to a computer delivers nothing.
        let path = _fixture._directory.path().join("secrets.json");
        let mut document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        document["secrets"][0]["computers"] = json!(["dev"]);
        fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
        assert!(crate::secrets::runtime_material("dev").unwrap().is_empty());
        assert!(crate::secrets::assigned_names("dev").unwrap().is_empty());
        assert!(crate::secrets::github_token_value(&id).is_ok());
    }

    fn entries(fixture_ids: &[(&str, bool)]) -> Vec<crate::secrets::TokenEntry> {
        fixture_ids
            .iter()
            .map(|(id, removing)| crate::secrets::TokenEntry {
                id: (*id).into(),
                name: (*id).into(),
                account: None,
                removing: *removing,
                value_id: format!("value-{id}"),
            })
            .collect()
    }
    fn token_values(
        pairs: &[(&str, &str)],
    ) -> Result<BTreeMap<String, crate::secrets::TokenValue>, String> {
        Ok(pairs
            .iter()
            .map(|(id, value)| {
                (
                    (*id).to_string(),
                    crate::secrets::TokenValue {
                        value_id: format!("value-{id}"),
                        value: (*value).to_string(),
                    },
                )
            })
            .collect())
    }
    fn rejected() -> Result<Value, String> {
        Err(format!(
            "{} Reconnect GitHub.",
            crate::github_http::CREDENTIALS_REJECTED
        ))
    }
    #[test]
    fn validation_disconnects_only_the_rejected_token() {
        let _fixture = Fixture::new();
        let values = || token_values(&[("a", "synthetic_good"), ("b", "synthetic_bad")]);
        let lookup = |token: &str| match token {
            "synthetic_good" => Ok(json!({"login":"alice"})),
            _ => rejected(),
        };
        let both = ["a", "b"];
        let verdicts = evaluate(&entries(&[("a", false), ("b", false)]), values, lookup);
        assert!(verdicts[0].status.as_ref().unwrap().connected);
        assert_eq!(verdicts[0].account.as_deref(), Some("alice"));
        assert!(!verdicts[1].status.as_ref().unwrap().connected);
        let message = verdicts[1]
            .status
            .as_ref()
            .unwrap()
            .message
            .clone()
            .unwrap();
        assert!(!message.contains("synthetic_bad"));
        assert_eq!(publish_verdicts(&verdicts, &both), ["a", "b"]);
        assert!(connected("a") && !connected("b"));
        // A later check where only b changes leaves a alone.
        let verdicts = evaluate(&entries(&[("a", false), ("b", false)]), values, |_| {
            Ok(json!({"login":"alice"}))
        });
        assert_eq!(publish_verdicts(&verdicts, &both), ["b"]);
        assert!(connected("a") && connected("b"));
        // A secret that vanished is forgotten.
        let verdicts = evaluate(&entries(&[("a", false)]), values, |_| {
            Ok(json!({"login":"alice"}))
        });
        assert_eq!(publish_verdicts(&verdicts, &["a"]), ["b"]);
        assert!(!connected("b"));
    }
    #[test]
    fn a_computer_whose_token_was_removed_is_marked_and_stays_without_a_token() {
        let _fixture = Fixture::new();
        let kept =
            crate::secrets::import_github_token("synthetic_token_a", "alice", "work").unwrap();
        connect(&[&kept]);
        let marked = with_missing_marks(&[
            token_policy("kept", Some(&kept)),
            token_policy("lost", Some("removed-id")),
            token_policy("none", None),
            json!({"computer":"signed-in","authenticationMethod":"oauth"}),
        ]);
        assert!(marked[0].get("tokenMissing").is_none());
        assert_eq!(marked[1]["tokenMissing"], true);
        assert!(marked[2].get("tokenMissing").is_none());
        assert!(marked[3].get("tokenMissing").is_none());
        // The policy keeps naming the removed token and gets no value for it.
        assert_eq!(marked[1]["tokenSecretId"], "removed-id");
        assert!(value_of(&marked[1]).is_err());
    }
    #[test]
    fn a_failed_lookup_keeps_the_previous_status() {
        let _fixture = Fixture::new();
        let values = || token_values(&[("a", "synthetic_a"), ("b", "synthetic_b")]);
        connect(&["a"]);
        statuses().insert(
            "b".into(),
            Status::disconnected("GitHub did not accept this token."),
        );
        for failure in [
            "Cannot reach GitHub.",
            "GitHub is limiting requests.",
            "GitHub access could not be updated.",
            "GitHub access update is waiting. Retrying in 2 seconds.",
        ] {
            let verdicts = evaluate(&entries(&[("a", false), ("b", false)]), values, |_| {
                Err(failure.to_string())
            });
            assert!(verdicts.iter().all(|v| v.status.is_none()), "{failure}");
            assert!(publish_verdicts(&verdicts, &["a", "b"]).is_empty());
            assert!(connected("a") && !connected("b"));
        }
        // An answer without an account says nothing about the token either.
        let verdicts = evaluate(&entries(&[("a", false)]), values, |_| Ok(json!({})));
        assert!(verdicts[0].status.is_none());
    }
    #[test]
    fn only_a_rejection_disconnects_and_only_a_rejection_is_final() {
        assert_eq!(
            validation("synthetic_token", |_| rejected()),
            Validation::Rejected(NOT_ACCEPTED.into())
        );
        assert_eq!(
            validation("synthetic_token", |_| Err("Cannot reach GitHub.".into())),
            Validation::Unknown
        );
        assert_eq!(
            validation("synthetic_token", |_| Ok(json!({"login":"alice"}))),
            Validation::Accepted("alice".into())
        );
        assert_eq!(
            validated_with("synthetic_token", |_| Err("Cannot reach GitHub.".into())),
            Err("Silo could not reach GitHub to check this token. Try again.".into())
        );
    }
    #[test]
    fn a_verdict_about_an_older_value_or_a_gone_secret_is_dropped() {
        let current = entries(&[("a", false), ("b", true)]);
        let verdict = |id: &str, value_id: &str, removing: bool| Verdict {
            id: id.into(),
            value_id: value_id.into(),
            removing,
            status: Some(Status::connected()),
            account: Some("alice".into()),
        };
        assert!(describes(&verdict("a", "value-a", false), &current));
        // The value was replaced during the lookup.
        assert!(!describes(&verdict("a", "older", false), &current));
        // The secret is gone, or started to be removed, or finished being removed meanwhile.
        assert!(!describes(&verdict("gone", "value-gone", false), &current));
        assert!(!describes(&verdict("b", "value-b", false), &current));
        assert!(describes(&verdict("b", "value-b", true), &current));
        assert!(!describes(&verdict("a", "value-a", true), &current));
    }
    #[test]
    fn a_publish_never_forgets_a_secret_that_is_still_there() {
        let _fixture = Fixture::new();
        connect(&["a"]);
        // No verdict was kept for "a" (its value changed during the lookup), but it exists.
        assert!(publish_verdicts(&[], &["a"]).is_empty());
        assert!(connected("a"));
    }
    #[test]
    fn a_check_with_an_unreachable_github_changes_nothing() {
        let _fixture = Fixture::new();
        let id = crate::secrets::import_github_token("synthetic_token_a", "alice", "work").unwrap();
        connect(&[&id]);
        use_test_lookup(Some(Box::new(|_| Err("Cannot reach GitHub.".into()))));
        CHECK_AT.store(0, Ordering::SeqCst);
        check();
        assert!(connected(&id));
        // The next check comes soon, not in five minutes.
        assert!(next_check() <= Duration::from_secs(30));
    }
    #[test]
    fn a_token_being_removed_is_disconnected_and_never_read() {
        let _fixture = Fixture::new();
        let verdicts = evaluate(
            &entries(&[("a", true)]),
            || panic!("a removed token's value was read"),
            |_| panic!("a removed token was sent to GitHub"),
        );
        assert!(!verdicts[0].status.as_ref().unwrap().connected);
        assert_eq!(connection("a", true)["state"], "disconnected");
    }
    #[test]
    fn an_unreadable_store_disconnects_every_live_token() {
        let _fixture = Fixture::new();
        let verdicts = evaluate(
            &entries(&[("a", false), ("b", false)]),
            || Err("Cannot access secrets in the system credential store.".into()),
            |_| panic!("nothing to send"),
        );
        assert!(verdicts
            .iter()
            .all(|v| !v.status.as_ref().unwrap().connected));
    }
    #[test]
    fn saving_a_token_asks_github_and_stores_nothing_when_it_refuses_or_cannot_say() {
        let _fixture = Fixture::new();
        for (answer, expected) in [
            (rejected(), "did not accept"),
            (Err("Cannot reach GitHub.".into()), "could not reach GitHub"),
        ] {
            use_test_lookup(Some(Box::new(move |_| answer.clone())));
            let request = crate::secrets::github_token_request(
                None,
                "work",
                Some(" synthetic_token_a ".into()),
            );
            let error = crate::secrets::save_secret(request, || Ok(())).unwrap_err();
            assert!(error.contains(expected), "{error}");
            assert!(!error.contains("synthetic_token_a"));
            assert!(crate::secrets::github_token_entries().unwrap().is_empty());
        }
    }
    #[test]
    fn a_token_secret_takes_a_label_and_no_computers_or_hosts() {
        let _fixture = Fixture::new();
        use_test_lookup(Some(Box::new(|_| {
            panic!("an invalid request reached GitHub")
        })));
        for name in ["", "-leading", "has space", "has.dot"] {
            let request =
                crate::secrets::github_token_request(None, name, Some("synthetic_token_a".into()));
            assert!(crate::secrets::save_secret(request, || Ok(())).is_err());
        }
    }

    /// The old entry, the saved removal intent and the computers' policies, with a step
    /// that can be made to fail.
    struct World {
        legacy: RefCell<Option<LegacyToken>>,
        removing: Cell<bool>,
        document: RefCell<Document>,
        fail: Cell<Option<&'static str>>,
    }
    impl World {
        fn new(token: Option<&str>) -> Self {
            World {
                legacy: RefCell::new(token.map(|token| LegacyToken {
                    token: token.into(),
                    account: "alice".into(),
                })),
                removing: Cell::new(false),
                document: RefCell::new(Document {
                    computers: vec![
                        token_policy("one", None),
                        token_policy("two", None),
                        json!({"computer":"three","authenticationMethod":"oauth"}),
                        json!({"computer":"four"}),
                    ],
                    ..Document::default()
                }),
                fail: Cell::new(None),
            }
        }
        fn step(&self, name: &'static str) -> Result<(), String> {
            match self.fail.get() {
                Some(failing) if failing == name => Err(format!("interrupted at {name}")),
                _ => Ok(()),
            }
        }
        fn run(&self) -> Result<(), String> {
            migrate_with(&Migration {
                removal_pending: &|| Ok(self.removing.get()),
                clear_removal: &|| {
                    self.step("clear")?;
                    self.removing.set(false);
                    Ok(())
                },
                read_legacy: &|| {
                    self.step("read")?;
                    Ok(self.legacy.borrow().as_ref().map(|legacy| LegacyToken {
                        token: legacy.token.clone(),
                        account: legacy.account.clone(),
                    }))
                },
                import: &|legacy| {
                    self.step("import")?;
                    crate::secrets::import_github_token(&legacy.token, &legacy.account, LEGACY_NAME)
                },
                repoint: &|id| {
                    self.step("repoint")?;
                    repoint(&mut self.document.borrow_mut(), id);
                    Ok(())
                },
                delete_legacy: &|| {
                    self.step("delete")?;
                    *self.legacy.borrow_mut() = None;
                    Ok(())
                },
            })
        }
        fn chosen(&self, computer: &str) -> Option<String> {
            self.document
                .borrow()
                .computers
                .iter()
                .find(|w| w["computer"] == computer)
                .and_then(|w| w["tokenSecretId"].as_str().map(str::to_owned))
        }
    }
    fn token_names() -> Vec<String> {
        crate::secrets::github_token_entries()
            .unwrap()
            .into_iter()
            .map(|entry| entry.name)
            .collect()
    }
    fn assert_migrated(world: &World) {
        let entries = crate::secrets::github_token_entries().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].account.as_deref(), Some("alice"));
        assert_eq!(
            crate::secrets::github_token_value(&entries[0].id).unwrap(),
            "synthetic_legacy"
        );
        assert_eq!(world.chosen("one"), Some(entries[0].id.clone()));
        assert_eq!(world.chosen("two"), Some(entries[0].id.clone()));
        assert_eq!(world.chosen("three"), None);
        assert_eq!(world.chosen("four"), None);
        assert!(world.legacy.borrow().is_none());
    }

    #[test]
    fn a_saved_token_becomes_a_secret_and_its_computers_point_at_it() {
        let _fixture = Fixture::new();
        let world = World::new(Some("synthetic_legacy"));
        world.run().unwrap();
        assert_migrated(&world);
        assert_eq!(token_names(), ["github-token"]);
    }
    #[test]
    fn migration_with_nothing_saved_changes_nothing() {
        let _fixture = Fixture::new();
        let world = World::new(None);
        world.run().unwrap();
        assert!(token_names().is_empty());
        assert_eq!(world.chosen("one"), None);
    }
    #[test]
    fn migration_can_be_repeated() {
        let _fixture = Fixture::new();
        let world = World::new(Some("synthetic_legacy"));
        world.run().unwrap();
        world.run().unwrap();
        world.run().unwrap();
        assert_migrated(&world);
    }
    #[test]
    fn migration_interrupted_at_any_step_finishes_at_the_next_start() {
        for step in ["read", "import", "repoint", "delete"] {
            let _fixture = Fixture::new();
            let world = World::new(Some("synthetic_legacy"));
            world.fail.set(Some(step));
            assert!(world.run().is_err(), "{step} did not interrupt");
            // Until the old entry is gone nothing has been lost.
            assert!(world.legacy.borrow().is_some());
            world.fail.set(None);
            world.run().unwrap();
            assert_migrated(&world);
        }
    }
    #[test]
    fn migration_picks_another_name_when_the_default_is_taken() {
        let _fixture = Fixture::new();
        crate::secrets::import_github_token("synthetic_other", "bob", LEGACY_NAME).unwrap();
        let world = World::new(Some("synthetic_legacy"));
        world.run().unwrap();
        world.run().unwrap();
        assert_eq!(token_names(), ["github-token", "github-token-2"]);
        let entries = crate::secrets::github_token_entries().unwrap();
        assert_eq!(world.chosen("one"), Some(entries[1].id.clone()));
        assert_eq!(
            crate::secrets::github_token_value(&entries[1].id).unwrap(),
            "synthetic_legacy"
        );
    }
    #[test]
    fn a_removal_pending_at_migration_is_honoured() {
        let _fixture = Fixture::new();
        let world = World::new(Some("synthetic_legacy"));
        world.removing.set(true);
        // An interrupted honouring leaves the intent for the next start.
        world.fail.set(Some("delete"));
        assert!(world.run().is_err());
        assert!(world.removing.get());
        world.fail.set(Some("clear"));
        assert!(world.run().is_err());
        assert!(world.legacy.borrow().is_none());
        world.fail.set(None);
        world.run().unwrap();
        assert!(!world.removing.get());
        assert!(token_names().is_empty());
        assert_eq!(world.chosen("one"), None);
        // With the intent cleared and nothing saved, later starts do nothing.
        world.run().unwrap();
        assert!(token_names().is_empty());
    }
    #[test]
    fn re_pointing_leaves_a_computer_that_already_names_a_token() {
        let mut d = Document {
            computers: vec![token_policy("one", Some("mine")), token_policy("two", None)],
            ..Document::default()
        };
        assert!(repoint(&mut d, "migrated"));
        assert_eq!(d.computers[0]["tokenSecretId"], "mine");
        assert_eq!(d.computers[1]["tokenSecretId"], "migrated");
        assert!(!repoint(&mut d, "migrated"));
    }
    #[test]
    fn the_single_token_status_follows_the_secret_named_github_token() {
        let _fixture = Fixture::new();
        assert_eq!(status(), json!({"state":"disconnected","saved":false}));
        let id =
            crate::secrets::import_github_token("synthetic_legacy", "alice", LEGACY_NAME).unwrap();
        assert_eq!(status()["saved"], true);
        assert_eq!(status()["state"], "disconnected");
        connect(&[&id]);
        assert_eq!(
            status(),
            json!({"state":"connected","saved":true,"account":"alice"})
        );
    }
    #[test]
    fn a_save_from_a_client_that_knows_only_token_mode_names_the_legacy_secret() {
        let _fixture = Fixture::new();
        let id =
            crate::secrets::import_github_token("synthetic_legacy", "alice", LEGACY_NAME).unwrap();
        // Until a client has used the single-token methods, nothing is chosen for a save.
        let unmarked = with_legacy_choice(json!({"computers":[token_policy("one", None)]}));
        assert!(unmarked["computers"][0]["tokenSecretId"].is_null());
        LEGACY_CLIENT.store(true, Ordering::SeqCst);
        let saved = with_legacy_choice(json!({"computers":[
            token_policy("one", None),
            token_policy("two", Some("other")),
            json!({"computer":"three","authenticationMethod":"oauth"}),
        ]}));
        assert_eq!(saved["computers"][0]["tokenSecretId"], id);
        assert_eq!(saved["computers"][1]["tokenSecretId"], "other");
        assert!(saved["computers"][2]["tokenSecretId"].is_null());
    }
}
