//! Personal tokens are independent of App OAuth. Only host runtime profiles receive the value.
use super::*;
use std::collections::HashMap;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PersonalToken {
    token: String,
    account: String,
}
static TOKEN_OPERATION: Mutex<()> = Mutex::new(());
static SECRET: SessionSecret<Option<PersonalToken>> = SessionSecret::new();
static STATUS: Mutex<Option<Value>> = Mutex::new(None);
static CHECK_AT: AtomicU64 = AtomicU64::new(0);
static APPLIED: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

pub(super) fn selected(policy: &Value) -> bool {
    policy["authenticationMethod"] == "token"
}
fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(
        crate::channel::current().keychain_service(crate::channel::Keychain::Github),
        "personal-token",
    )
    .map_err(|_| "The system credential store is unavailable.".into())
}
fn read() -> Result<Option<PersonalToken>, String> {
    SECRET.read(|| match entry()?.get_password() {
        Ok(value) => {
            serde_json::from_str(&value).map_err(|_| "Stored GitHub token is invalid.".into())
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err("Cannot read the GitHub token from the system credential store.".into()),
    })
}
pub(super) fn status() -> Value {
    STATUS
        .lock()
        .ok()
        .and_then(|s| s.clone())
        .unwrap_or_else(|| json!({"state":"disconnected","saved":false}))
}
pub(super) fn connected() -> bool {
    status()["state"] == "connected"
}
fn publish(value: Value) -> bool {
    let mut state = STATUS.lock().unwrap_or_else(|e| e.into_inner());
    let changed = state.as_ref() != Some(&value);
    *state = Some(value);
    changed
}
fn validated(token: &str) -> Result<PersonalToken, String> {
    validated_with(token, |token| github(token, "/user"))
}
fn validated_with(
    token: &str,
    lookup: impl FnOnce(&str) -> Result<Value, String>,
) -> Result<PersonalToken, String> {
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
    let account = user["login"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 100)
        .ok_or("GitHub did not identify this token's account.")?;
    Ok(PersonalToken {
        token: token.into(),
        account: account.into(),
    })
}
pub(super) fn value() -> Result<String, String> {
    if !connected() {
        return Err(
            "The personal token is disconnected. Add or replace it in GitHub settings.".into(),
        );
    }
    read()?
        .map(|t| t.token)
        .ok_or("Add a personal token in GitHub settings.".into())
}
fn fingerprint(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}
/// The connected token's fingerprint from memory only, like `value()` without the store.
fn current_fingerprint() -> Option<String> {
    if !connected() {
        return None;
    }
    match SECRET.peek() {
        Some(Ok(Some(token))) => Some(fingerprint(&token.token)),
        _ => None,
    }
}
fn applied() -> &'static Mutex<HashMap<String, String>> {
    APPLIED.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn apply(name: &str, revision: u64) -> Result<(), String> {
    // The credential store can wait on a permission prompt; never read it under STATE.
    let token = value();
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
            let profile = json!({"version":2,"owners":[],"personalToken":token});
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

/// Disable access is a global kill switch: no computer keeps the personal token while it is off.
fn keeps_token(d: &Document, name: &str, current: Option<&String>, attached: &String) -> bool {
    d.access_enabled
        && d.computers
            .iter()
            .any(|w| w["computer"] == name && selected(w))
        && current == Some(attached)
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
    let current = current_fingerprint();
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
            (!keeps_token(d, name, current.as_ref(), attached))
                .then(|| (name.to_owned(), key.clone()))
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
fn changed(removing: Option<bool>) -> Result<(), String> {
    let _state = serialize(&STATE);
    let mut d = load()?;
    d.revision = next_policy_revision(d.revision)?;
    if let Some(removing) = removing {
        d.personal_token_removing = removing;
    }
    let names: Vec<_> = d
        .computers
        .iter()
        .filter(|w| selected(w))
        .filter_map(|w| w["computer"].as_str().map(str::to_owned))
        .collect();
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
/// Time until `check` validates the token again, so the worker can sleep until then.
pub(super) fn next_check() -> Duration {
    Duration::from_secs(CHECK_AT.load(Ordering::SeqCst).saturating_sub(now()))
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
    let removing = load().map(|d| d.personal_token_removing).unwrap_or(true);
    let next = if removing {
        json!({"state":"disconnected","saved":true,"message":"Token removal is pending. Retry Remove token."})
    } else {
        match read() {
            Ok(Some(token)) => match validated(&token.token) {
                Ok(token) => json!({"state":"connected","saved":true,"account":token.account}),
                Err(message) => {
                    CHECK_AT.store(now() + 30, Ordering::SeqCst);
                    json!({"state":"disconnected","saved":true,"message":message})
                }
            },
            Ok(None) => json!({"state":"disconnected","saved":false}),
            Err(message) => json!({"state":"disconnected","saved":true,"message":message}),
        }
    };
    if publish(next) {
        if let Err(message) = changed(None) {
            // Keep the failed detachment visible and retry it through the normal worker.
            let _state = serialize(&STATE);
            if let Ok(mut d) = load() {
                for name in d
                    .computers
                    .iter()
                    .filter(|w| selected(w))
                    .filter_map(|w| w["computer"].as_str())
                {
                    d.access_errors.insert(name.into(), message.clone());
                }
                let _ = save(&d);
            }
        }
    }
}

fn save_validated_token(
    secret: &SessionSecret<Option<PersonalToken>>,
    token: PersonalToken,
    persist: impl FnOnce() -> Result<(), String>,
    connected: impl FnOnce(&PersonalToken) -> Result<(), String>,
) -> Result<(), String> {
    secret.replace(Some(token.clone()), persist)?;
    connected(&token)
}

pub fn save_github_personal_token(token: String) -> Result<Value, String> {
    let _network = serialize(&TOKEN_OPERATION);
    // Saving a token is an explicit retry; never leave validation blocked by earlier failures.
    crate::github_http::reset_bearer_retries(token.trim());
    let token = validated(token.trim())?;
    SECRET.retry();
    save_validated_token(
        &SECRET,
        token.clone(),
        || {
            entry()?
                .set_password(
                    &serde_json::to_string(&token).map_err(|_| "Cannot encode GitHub token.")?,
                )
                .map_err(|_| "Cannot save GitHub token in the system credential store.".into())
        },
        |token| {
            publish(json!({"state":"connected","saved":true,"account":token.account}));
            CHECK_AT.store(now() + 300, Ordering::SeqCst);
            changed(Some(false))
        },
    )?;

    snapshot()
}

pub fn remove_github_personal_token() -> Result<Value, String> {
    let _network = serialize(&TOKEN_OPERATION);
    // Detach before deletion so a storage failure cannot leave guest access silently enabled.
    publish(json!({"state":"disconnected","saved":true,"message":"Removing personal token."}));
    changed(Some(true))?;
    SECRET.retry();
    SECRET.write(None, || match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err("Cannot remove GitHub token from the system credential store.".into()),
    })?;
    publish(json!({"state":"disconnected","saved":false}));
    CHECK_AT.store(now() + 300, Ordering::SeqCst);
    changed(Some(false))?;
    snapshot()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_personal_token_replacement_preserves_cached_and_stored_identity() {
        let cache = SessionSecret::new();
        let old = PersonalToken {
            token: "github_pat_old".into(),
            account: "old-account".into(),
        };
        let new = PersonalToken {
            token: "github_pat_new".into(),
            account: "new-account".into(),
        };
        let stored = std::cell::RefCell::new(Some(old.clone()));
        cache.read(|| Ok(stored.borrow().clone())).unwrap();
        assert!(cache
            .replace(Some(new.clone()), || Err("store denied".into()))
            .is_err());
        assert!(cache.peek() == Some(Ok(Some(old.clone()))));
        assert!(
            cache
                .read(|| panic!("replacement invalidated the cached identity"))
                .unwrap()
                == Some(old.clone())
        );
        assert!(*stored.borrow() == Some(old));
        cache
            .flush(|_| panic!("failed replacement was queued for later activation"))
            .unwrap();
        cache
            .replace(Some(new.clone()), || {
                *stored.borrow_mut() = Some(new.clone());
                Ok(())
            })
            .unwrap();
        assert!(cache.peek() == Some(Ok(Some(new.clone()))));
        assert!(*stored.borrow() == Some(new));
    }

    #[test]
    fn failed_personal_token_replacement_preserves_identity_until_retry() {
        let secret = SessionSecret::new();
        let entry =
            keyring::Entry::new_with_credential(Box::new(keyring::mock::MockCredential::default()));
        let a = PersonalToken {
            token: "token_a".into(),
            account: "account_a".into(),
        };
        let b = PersonalToken {
            token: "token_b".into(),
            account: "account_b".into(),
        };
        let status =
            std::cell::RefCell::new(json!({"state":"connected","saved":true,"account":a.account}));
        let applied = std::cell::RefCell::new(fingerprint(&a.token));
        let revision = std::cell::Cell::new(0);
        entry
            .set_password(&serde_json::to_string(&a).unwrap())
            .unwrap();
        secret.read(|| Ok(Some(a.clone()))).unwrap();
        let persist = || {
            entry
                .set_password(&serde_json::to_string(&b).unwrap())
                .map_err(|_| "denied".into())
        };
        let connected = |token: &PersonalToken| {
            *status.borrow_mut() =
                json!({"state":"connected","saved":true,"account":token.account});
            *applied.borrow_mut() = fingerprint(&token.token);
            revision.set(revision.get() + 1);
            Ok(())
        };
        entry
            .get_credential()
            .downcast_ref::<keyring::mock::MockCredential>()
            .unwrap()
            .set_error(keyring::Error::NoStorageAccess(Box::new(
                std::io::Error::other("denied"),
            )));
        assert!(save_validated_token(&secret, b.clone(), persist, connected).is_err());
        assert_eq!(
            secret
                .read(|| panic!("unexpected store read"))
                .unwrap()
                .unwrap()
                .token,
            a.token
        );
        assert_eq!(status.borrow()["account"], a.account);
        assert_eq!(status.borrow()["saved"], true);
        assert_eq!(*applied.borrow(), fingerprint(&a.token));
        assert_eq!(revision.get(), 0);
        let reloaded: PersonalToken = serde_json::from_str(&entry.get_password().unwrap()).unwrap();
        assert_eq!(reloaded.token, a.token);
        secret.retry();
        save_validated_token(&secret, b.clone(), persist, connected).unwrap();
        assert_eq!(
            secret
                .read(|| panic!("unexpected store read"))
                .unwrap()
                .unwrap()
                .token,
            b.token
        );
        assert_eq!(status.borrow()["account"], b.account);
        assert_eq!(*applied.borrow(), fingerprint(&b.token));
        assert_eq!(revision.get(), 1);
        let restarted = SessionSecret::new();
        let reloaded = restarted
            .read(|| {
                serde_json::from_str::<PersonalToken>(&entry.get_password().unwrap())
                    .map(Some)
                    .map_err(|_| "invalid".into())
            })
            .unwrap()
            .unwrap();
        assert_eq!(reloaded.token, b.token);
    }
    #[test]
    fn empty_account_connects_without_repository_discovery() {
        let token = validated_with("github_pat_synthetic", |_| {
            Ok(json!({"login":"new-user","public_repos":0}))
        })
        .unwrap();
        assert_eq!(token.account, "new-user");
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
    fn method_switch_requires_only_its_selected_connection() {
        let token = json!({"authenticationMethod":"token"});
        let oauth = json!({"authenticationMethod":"oauth"});
        assert!(validate_method_change(None, &token, true, false).is_err());
        assert!(validate_method_change(None, &token, false, true).is_ok());
        assert!(validate_method_change(Some(&token), &oauth, false, true).is_err());
        assert!(validate_method_change(Some(&token), &oauth, true, false).is_ok());
        // Existing unavailable choices are preserved during unrelated identity edits.
        assert!(validate_method_change(Some(&token), &token, false, false).is_ok());
        assert!(validate_method_change(None, &oauth, false, false).is_ok());
    }
    #[test]
    fn disable_access_detaches_personal_token_computers() {
        let fingerprint = "attached".to_string();
        let mut d = Document {
            access_enabled: true,
            computers: vec![json!({"computer":"dev","authenticationMethod":"token"})],
            ..Document::default()
        };
        assert!(keeps_token(&d, "dev", Some(&fingerprint), &fingerprint));
        d.access_enabled = false;
        assert!(!keeps_token(&d, "dev", Some(&fingerprint), &fingerprint));
    }
    #[test]
    fn legacy_policies_stay_oauth_and_token_policy_has_no_oauth_scopes() {
        assert!(!selected(&json!({})));
        assert!(selected(&json!({"authenticationMethod":"token"})));
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
}
