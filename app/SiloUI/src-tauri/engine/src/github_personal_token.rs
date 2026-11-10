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
        None => json!({"state":"disconnected","message":"Waiting for GitHub to confirm this token."}),
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
pub(super) fn connected(id: &str) -> bool {
    statuses().get(id).is_some_and(|status| status.connected)
}
/// The computers whose GitHub access uses each token secret.
pub(crate) fn users() -> BTreeMap<String, Vec<String>> {
    let mut users: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for policy in load().map(|d| d.computers).unwrap_or_default() {
        if let (Some(id), Some(computer)) = (chosen(&policy), policy["computer"].as_str()) {
            users.entry(id.to_owned()).or_default().push(computer.to_owned());
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
fn validated_with(
    token: &str,
    lookup: impl FnOnce(&str) -> Result<Value, String>,
) -> Result<String, String> {
    if token.is_empty()
        || token.len() > 4096
        || !token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err("Enter a valid GitHub personal access token.".into());
    }
    let user = lookup(token).map_err(|_| {
        "Could not validate the GitHub token. Check its validity and your connection.".to_string()
    })?;
    user["login"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 100)
        .map(str::to_owned)
        .ok_or("GitHub did not identify this token's account.".into())
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
    status: Status,
    account: Option<String>,
}
/// Validates every token secret. `values` is read only when some token is live, and
/// `lookup` answers GitHub's `/user`. A failure of one token never changes another's verdict.
fn evaluate(
    entries: &[crate::secrets::TokenEntry],
    values: impl FnOnce() -> Result<BTreeMap<String, String>, String>,
    lookup: impl Fn(&str) -> Result<Value, String>,
) -> Vec<Verdict> {
    let live = entries.iter().any(|entry| !entry.removing);
    let values = if live { Some(values()) } else { None };
    entries
        .iter()
        .map(|entry| {
            let (status, account) = if entry.removing {
                (
                    Status::disconnected("Removal is pending. Retry removing this token."),
                    None,
                )
            } else {
                match values.as_ref() {
                    Some(Ok(values)) => match values.get(&entry.id) {
                        Some(token) => match validated_with(token, &lookup) {
                            Ok(account) => (Status::connected(), Some(account)),
                            Err(message) => (Status::disconnected(message), None),
                        },
                        None => (
                            Status::disconnected("The token's value is missing. Replace it."),
                            None,
                        ),
                    },
                    Some(Err(message)) => (Status::disconnected(message.clone()), None),
                    None => (Status::disconnected("The token could not be read."), None),
                }
            };
            Verdict {
                id: entry.id.clone(),
                status,
                account,
            }
        })
        .collect()
}
/// Records verdicts and returns the ids whose status changed (a vanished secret counts).
fn publish_verdicts(verdicts: &[Verdict]) -> Vec<String> {
    let mut statuses = statuses();
    let mut changed: Vec<String> = statuses
        .keys()
        .filter(|id| !verdicts.iter().any(|v| &v.id == *id))
        .cloned()
        .collect();
    for id in &changed {
        statuses.remove(id);
    }
    for verdict in verdicts {
        if statuses.get(&verdict.id) != Some(&verdict.status) {
            statuses.insert(verdict.id.clone(), verdict.status.clone());
            changed.push(verdict.id.clone());
        }
    }
    changed
}
/// Called by the existing serialized worker. Snapshot reads never touch secure storage.
pub(super) fn check() {
    let Some(_operation) = try_serialize(&TOKEN_OPERATION) else {
        return;
    };
    if now() < CHECK_AT.load(Ordering::SeqCst) {
        return;
    }
    CHECK_AT.store(now() + 300, Ordering::SeqCst);
    let Ok(entries) = crate::secrets::github_token_entries() else {
        CHECK_AT.store(now() + 30, Ordering::SeqCst);
        return;
    };
    let verdicts = evaluate(&entries, crate::secrets::github_token_values, user_lookup);
    if verdicts
        .iter()
        .any(|v| !v.status.connected && !entries.iter().any(|e| e.id == v.id && e.removing))
    {
        CHECK_AT.store(now() + 30, Ordering::SeqCst);
    }
    let mut account_changed = false;
    for verdict in &verdicts {
        if let Some(account) = &verdict.account {
            account_changed |=
                crate::secrets::set_github_token_account(&verdict.id, account).unwrap_or(false);
        }
    }
    let changed_ids = publish_verdicts(&verdicts);
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

/// A token secret was saved with a new value that GitHub accepted: its computers pick it up.
pub(crate) fn secret_saved(id: &str) -> Result<(), String> {
    statuses().insert(id.to_owned(), Status::connected());
    CHECK_AT.store(now() + 300, Ordering::SeqCst);
    changed(Some(&[id]))
}
/// The user asked to try a token again.
pub(crate) fn secret_retry(id: &str) -> Result<(), String> {
    CHECK_AT.store(0, Ordering::SeqCst);
    statuses().remove(id);
    changed(Some(&[id]))
}
/// A token secret is being removed: detach every computer using it before its value goes.
pub(crate) fn secret_removing(id: &str) -> Result<(), String> {
    statuses().insert(
        id.to_owned(),
        Status::disconnected("Removing this token."),
    );
    changed(Some(&[id]))
}
/// A token secret and its value are gone.
pub(crate) fn secret_removed(id: &str) {
    statuses().remove(id);
    let _ = changed(Some(&[id]));
}

/// A save from a client that knows only "token mode" names no token: it means the secret
/// earlier versions managed, when there is one.
pub(super) fn with_legacy_choice(mut configuration: Value) -> Value {
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
    let existing = crate::secrets::github_token_named(LEGACY_NAME)?;
    crate::secrets::save_secret(
        crate::secrets::github_token_request(existing, LEGACY_NAME, Some(token)),
        || Ok(()),
    )?;
    snapshot()
}

/// Earlier versions' `github.personalToken.remove`: removes the secret named `github-token`.
pub fn remove_github_personal_token() -> Result<Value, String> {
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
        assert!(validate_method_change(None, &none, true, &connected).is_err());
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
            })
            .collect()
    }
    #[test]
    fn validation_disconnects_only_the_rejected_token() {
        let _fixture = Fixture::new();
        let values = || {
            Ok(BTreeMap::from([
                ("a".to_string(), "synthetic_good".to_string()),
                ("b".to_string(), "synthetic_bad".to_string()),
            ]))
        };
        let lookup = |token: &str| match token {
            "synthetic_good" => Ok(json!({"login":"alice"})),
            _ => Err("401 synthetic_bad rejected".to_string()),
        };
        let verdicts = evaluate(&entries(&[("a", false), ("b", false)]), values, lookup);
        assert!(verdicts[0].status.connected);
        assert_eq!(verdicts[0].account.as_deref(), Some("alice"));
        assert!(!verdicts[1].status.connected);
        let message = verdicts[1].status.message.clone().unwrap();
        assert!(!message.contains("synthetic_bad"));
        assert_eq!(publish_verdicts(&verdicts), ["a", "b"]);
        assert!(connected("a") && !connected("b"));
        // A later check where only b changes leaves a alone.
        let verdicts = evaluate(&entries(&[("a", false), ("b", false)]), values, |_| {
            Ok(json!({"login":"alice"}))
        });
        assert_eq!(publish_verdicts(&verdicts), ["b"]);
        assert!(connected("a") && connected("b"));
        // A secret that vanished is forgotten.
        let verdicts = evaluate(&entries(&[("a", false)]), values, |_| {
            Ok(json!({"login":"alice"}))
        });
        assert_eq!(publish_verdicts(&verdicts), ["b"]);
        assert!(!connected("b"));
    }
    #[test]
    fn a_token_being_removed_is_disconnected_and_never_read() {
        let _fixture = Fixture::new();
        let verdicts = evaluate(
            &entries(&[("a", true)]),
            || panic!("a removed token's value was read"),
            |_| panic!("a removed token was sent to GitHub"),
        );
        assert!(!verdicts[0].status.connected);
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
        assert!(verdicts.iter().all(|v| !v.status.connected));
    }
    #[test]
    fn saving_a_token_asks_github_and_stores_nothing_when_it_refuses() {
        let _fixture = Fixture::new();
        use_test_lookup(Some(Box::new(|_| Err("401".into()))));
        let request = crate::secrets::github_token_request(
            None,
            "work",
            Some(" synthetic_token_a ".into()),
        );
        let error = crate::secrets::save_secret(request, || Ok(())).unwrap_err();
        assert!(error.contains("Could not validate"));
        assert!(!error.contains("synthetic_token_a"));
        assert!(crate::secrets::github_token_entries().unwrap().is_empty());
    }
    #[test]
    fn a_token_secret_takes_a_label_and_no_computers_or_hosts() {
        let _fixture = Fixture::new();
        use_test_lookup(Some(Box::new(|_| panic!("an invalid request reached GitHub"))));
        for name in ["", "-leading", "has space", "has.dot"] {
            let request = crate::secrets::github_token_request(
                None,
                name,
                Some("synthetic_token_a".into()),
            );
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
        let id = crate::secrets::import_github_token("synthetic_legacy", "alice", LEGACY_NAME)
            .unwrap();
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
        let id = crate::secrets::import_github_token("synthetic_legacy", "alice", LEGACY_NAME)
            .unwrap();
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
