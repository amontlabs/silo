//! Non-interactive access to the system credential store.
//!
//! The engine runs unattended (launch migration, timers, startup), so it must never wait on
//! a Keychain password dialog. `forbid_interaction` makes every Keychain call of the process
//! fail with `errSecInteractionNotAllowed` instead of prompting, and `describe` turns that
//! failure into one stable message that the dispatcher maps to the `credential_access_denied`
//! error code. Callers treat it as "skip and retry later", never as a reason to stop.

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

/// Whether a keyring failure is a Keychain refusal.
pub fn is_access_denied(error: &keyring::Error) -> bool {
    #[cfg(target_os = "macos")]
    {
        let source = match error {
            keyring::Error::PlatformFailure(source) | keyring::Error::NoStorageAccess(source) => {
                source
            }
            _ => return false,
        };
        source
            .downcast_ref::<keychain_status::base::Error>()
            .is_some_and(|error| is_denied_status(error.code()))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = error;
        false
    }
}

/// The message to report for a failed credential-store call: the stable refusal message when
/// the Keychain refused, `fallback` otherwise.
pub fn describe(error: &keyring::Error, fallback: &str) -> String {
    if is_access_denied(error) {
        ACCESS_DENIED_MESSAGE.to_owned()
    } else {
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
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn forbidding_interaction_sticks_for_the_process() {
        forbid_interaction().unwrap();
        assert!(interaction_forbidden());
    }
}
