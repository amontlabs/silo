//! Non-interactive access to the system credential store.
//!
//! The engine runs unattended (launch migration, timers, startup), so it must never wait on
//! a Keychain password dialog. `forbid_interaction` makes every Keychain call of the process
//! fail with `errSecInteractionNotAllowed` instead of prompting, and `describe` turns that
//! failure into one stable message that the dispatcher maps to the `credential_access_denied`
//! error code. Callers treat it as "skip and retry later", never as a reason to stop.
//!
//! On a Linux host the engine's file store (`credential_file_store`) refuses a file or folder
//! it will not use (a link, another account's, readable by others). That refusal gets its own
//! stable message, naming the file and never the value, with the same error code, and
//! `engine.credentials.status` reports the item as refused.

/// The message of a credential-store refusal. Stable: the dispatcher matches it.
pub const ACCESS_DENIED_MESSAGE: &str =
    "Allow Silo's engine to use its saved sign-in in the macOS Keychain. (credential_access_denied)";

/// errSecAuthFailed, errSecInteractionNotAllowed and errUserCanceled: the item exists but the
/// engine's signature is not in its access list, or the system would have had to ask.
const DENIED_STATUSES: [i32; 3] = [-25293, -25308, -128];

/// Whether an OSStatus means the Keychain refused (or would have asked) rather than failed.
pub fn is_denied_status(status: i32) -> bool {
    DENIED_STATUSES.contains(&status)
}

/// Whether `message` is (or contains) the credential-store refusal.
pub fn is_access_denied_message(message: &str) -> bool {
    message.contains("credential_access_denied")
}

/// The start of the message of a file-store refusal on a Linux host. Stable: it is followed by
/// the refused path and reason, then `(credential_access_denied)`.
#[cfg(target_os = "linux")]
pub const FILE_STORE_REFUSED: &str = "Silo's engine refused its credential file";

/// The file store's refusal inside a keyring failure, on a Linux host.
#[cfg(target_os = "linux")]
pub fn file_store_refusal(
    error: &keyring::Error,
) -> Option<&crate::credential_file_store::StoreRefusal> {
    match error {
        keyring::Error::NoStorageAccess(source) => source.downcast_ref(),
        _ => None,
    }
}

/// The stable message of a file-store refusal: the path and reason, never a value.
#[cfg(target_os = "linux")]
pub fn file_store_refusal_message(refusal: &crate::credential_file_store::StoreRefusal) -> String {
    format!(
        "{FILE_STORE_REFUSED} {refusal}. Only the engine's account may own it, with mode 0600 \
         for files and 0700 for folders. (credential_access_denied)"
    )
}

/// Whether a keyring failure is a Keychain refusal, or on Linux a file-store refusal.
pub fn is_access_denied(error: &keyring::Error) -> bool {
    #[cfg(target_os = "macos")]
    {
        let source = match error {
            keyring::Error::PlatformFailure(source) | keyring::Error::NoStorageAccess(source) => {
                source
            }
            _ => return false,
        };
        // keyring 3.6 links two security-framework versions: the legacy keychain calls report
        // the 2.x error, the SecItem calls the 3.x one. The status is the same.
        source
            .downcast_ref::<keychain_status::base::Error>()
            .map(|error| error.code())
            .or_else(|| {
                source
                    .downcast_ref::<keychain_status_v3::base::Error>()
                    .map(|error| error.code())
            })
            .is_some_and(is_denied_status)
    }
    #[cfg(target_os = "linux")]
    {
        file_store_refusal(error).is_some()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = error;
        false
    }
}

/// The message to report for a failed credential-store call: the stable refusal message when
/// the Keychain refused, `fallback` otherwise.
pub fn describe(error: &keyring::Error, fallback: &str) -> String {
    #[cfg(target_os = "linux")]
    if let Some(refusal) = file_store_refusal(error) {
        return file_store_refusal_message(refusal);
    }
    if is_access_denied(error) {
        ACCESS_DENIED_MESSAGE.to_owned()
    } else {
        // The caller reports a generic message; the engine log keeps the reason (a status code
        // or a variant, never a value) so a failure that is not a refusal can be told apart.
        eprintln!("silo-engine: credential store call failed (not a refusal): {error:?}");
        fallback.to_owned()
    }
}

/// Makes every Keychain call of this process fail instead of showing UI. Called once by the
/// unattended engine process at startup.
pub fn forbid_interaction() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: a plain Security framework call with a Boolean argument.
        let status =
            unsafe { keychain_status_sys::keychain::SecKeychainSetUserInteractionAllowed(0) };
        if status != 0 {
            return Err(format!(
                "Could not make Keychain access non-interactive (OSStatus {status})."
            ));
        }
    }
    Ok(())
}

/// Lets Keychain calls of this process prompt again. Only the user-started access repair
/// (`credential_access`) calls this, and it forbids prompts again when it ends.
pub fn allow_interaction() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: a plain Security framework call with a Boolean argument.
        let status =
            unsafe { keychain_status_sys::keychain::SecKeychainSetUserInteractionAllowed(1) };
        if status != 0 {
            return Err(format!(
                "Could not allow Keychain access to ask (OSStatus {status})."
            ));
        }
    }
    Ok(())
}

/// Whether Keychain calls of this process are forbidden from prompting.
#[cfg(target_os = "macos")]
pub fn interaction_forbidden() -> bool {
    let mut allowed: u8 = 1;
    // SAFETY: the pointer is to a live Boolean.
    let status = unsafe {
        keychain_status_sys::keychain::SecKeychainGetUserInteractionAllowed(&mut allowed)
    };
    status == 0 && allowed == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denied_statuses_are_the_keychain_refusals() {
        for status in [-25293, -25308, -128] {
            assert!(is_denied_status(status));
        }
        assert!(!is_denied_status(-25300), "item not found is not a refusal");
        assert!(!is_denied_status(0));
    }

    #[test]
    fn only_refusals_get_the_stable_message() {
        assert!(is_access_denied_message(ACCESS_DENIED_MESSAGE));
        assert!(is_access_denied_message(&format!(
            "wrapped: {ACCESS_DENIED_MESSAGE}"
        )));
        assert!(!is_access_denied_message("Cannot read GitHub credentials."));
        assert_eq!(describe(&keyring::Error::NoEntry, "fallback"), "fallback");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_file_store_refusal_has_its_own_stable_message_without_the_value() {
        use crate::credential_file_store::FileCredentialBuilder;
        use keyring::credential::CredentialBuilderApi;
        use std::os::unix::fs::PermissionsExt;

        let home = tempfile::tempdir().unwrap();
        let item = keyring::Entry::new_with_credential(
            FileCredentialBuilder::new(home.path().to_path_buf())
                .build(None, "service", "account")
                .unwrap(),
        );
        item.set_password("secret-value-123").unwrap();
        let path = crate::credential_file_store::credential_path(&item).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let error = item.get_password().unwrap_err();
        assert!(is_access_denied(&error));
        let message = describe(&error, "fallback");
        assert!(message.starts_with(FILE_STORE_REFUSED), "{message}");
        assert!(message.contains(&path.display().to_string()), "{message}");
        assert!(is_access_denied_message(&message));
        assert!(!message.contains("secret-value-123"));
        // Other failures keep the caller's message.
        let other = keyring::Error::PlatformFailure(Box::new(std::io::Error::other("disk")));
        assert!(!is_access_denied(&other));
        assert_eq!(describe(&other, "fallback"), "fallback");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_keychain_refusal_is_recognised_and_other_failures_are_not() {
        let denied =
            keyring::Error::PlatformFailure(Box::new(keychain_status::base::Error::from(-25308)));
        assert!(is_access_denied(&denied));
        assert_eq!(describe(&denied, "fallback"), ACCESS_DENIED_MESSAGE);
        let other =
            keyring::Error::PlatformFailure(Box::new(keychain_status::base::Error::from(-25299)));
        assert!(!is_access_denied(&other));
        let locked =
            keyring::Error::NoStorageAccess(Box::new(keychain_status::base::Error::from(-128)));
        assert!(is_access_denied(&locked));
        // The SecItem API reports the other security-framework version: the real refusal of
        // the owner's item showed up as `PlatformFailure(Error { code: -25293 })` of this type.
        let sec_item = keyring::Error::PlatformFailure(Box::new(
            keychain_status_v3::base::Error::from_code(-25293),
        ));
        assert!(is_access_denied(&sec_item));
        assert_eq!(describe(&sec_item, "fallback"), ACCESS_DENIED_MESSAGE);
        let sec_item_other = keyring::Error::PlatformFailure(Box::new(
            keychain_status_v3::base::Error::from_code(-25299),
        ));
        assert!(!is_access_denied(&sec_item_other));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn forbidding_interaction_sticks_for_the_process() {
        forbid_interaction().unwrap();
        assert!(interaction_forbidden());
    }
}
