//! The fork-time base: noted when a fork is created, captured when the fork first starts,
//! right after its quiet step is confirmed and before the start reports ready.

use std::sync::{Arc, Mutex, OnceLock};

use super::{
    collect::{self, Options, TargetRunner},
    store::{Slot, Store},
};
use crate::{
    context::EngineContext,
    guest::{self, Target},
    runtime::RuntimePaths,
};

fn installed() -> &'static Mutex<Option<Arc<dyn EngineContext>>> {
    static CONTEXT: OnceLock<Mutex<Option<Arc<dyn EngineContext>>>> = OnceLock::new();
    CONTEXT.get_or_init(Default::default)
}

/// Remembers the engine the start paths capture for. The host installs it at boot and every
/// fork creation refreshes it.
pub fn install(context: Arc<dyn EngineContext>) {
    *installed()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(context);
}

fn context() -> Option<Arc<dyn EngineContext>> {
    installed()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// What a baseline key starts with when the fork came from a recipe.
pub const RECIPE_KEY_PREFIX: &str = "recipe:";

/// The key the baseline of a fork is cached under: the recipe it was built from, else the
/// checkpoint it was made from, else the source computer when it was made from the current
/// state. A recipe's key survives the checkpoint being rebuilt, so every fork of one recipe
/// shares a baseline.
pub fn source_key(
    checkpoint_id: Option<&str>,
    source_computer_id: &str,
    recipe_hash: Option<&str>,
) -> String {
    match recipe_hash {
        Some(hash) => format!("{RECIPE_KEY_PREFIX}{hash}"),
        None => checkpoint_id.unwrap_or(source_computer_id).to_owned(),
    }
}

/// Notes at fork creation that `new_name` wants a base at its first start.
pub fn expect(context: Arc<dyn EngineContext>, new_name: &str, source: &str, computer: &str) {
    let store = Store::for_engine(context.as_ref());
    install(context);
    if let Err(error) = store
        .put_pending(new_name, source)
        .and_then(|()| store.put_pending_origin(new_name, computer))
    {
        eprintln!("The fork-time record of {new_name} could not be scheduled: {error}");
    }
}

/// The checkpoint a fork was actually made from, known once the fork's source is resolved
/// (for a fork of the current state, the checkpoint taken for it). It replaces the key noted
/// at fork creation, so a later fork of the same computer never shares this baseline.
pub fn forked_from(new_name: &str, checkpoint_id: &str) {
    let Some(context) = context() else { return };
    let store = Store::for_engine(context.as_ref());
    if let Some(noted) = store.take_pending(new_name) {
        // A recipe's key stays: it is what the baseline is shared under.
        let key = if noted.starts_with(RECIPE_KEY_PREFIX) {
            noted.as_str()
        } else {
            checkpoint_id
        };
        if let Err(error) = store.put_pending(new_name, key) {
            eprintln!("The fork-time record of {new_name} could not be updated: {error}");
        }
    }
}

/// Withdraws the note of a fork that was never created.
pub fn withdraw(context: &dyn EngineContext, new_name: &str) {
    let store = Store::for_engine(context);
    let _ = store.take_pending(new_name);
    let _ = store.take_pending_origin(new_name);
}

/// A Linux fork has just been confirmed quiet.
pub fn captured_after_quiet(paths: &RuntimePaths, name: &str, id: &str) {
    let Some(context) = context() else { return };
    let target = guest::resolve(Ok(paths.clone()), &context.paths().app_data, id);
    capture_with(context.as_ref(), target, name, id);
}

/// A macOS fork has just finished its first boot.
pub fn captured_after_first_boot(name: &str, id: &str) {
    let Some(context) = context() else { return };
    let target = guest::resolve(
        Err("This device has no Linux runtime.".into()),
        &context.paths().app_data,
        id,
    );
    capture_with(context.as_ref(), target, name, id);
}

/// Takes the base for `id` if `name` is a fork waiting for one. A failure is recorded for
/// `changes.diff` to report, and never fails the start.
pub fn capture_with(
    context: &dyn EngineContext,
    target: Result<Target, guest::GuestError>,
    name: &str,
    id: &str,
) {
    let store = Store::for_engine(context);
    let Some(source) = store.take_pending(name) else {
        return;
    };
    let origin = store.take_pending_origin(name);
    // A capture that could not run leaves the note, so the next start tries again.
    let keep_note = |source: &str| {
        let _ = store.put_pending(name, source);
        if let Some(origin) = &origin {
            let _ = store.put_pending_origin(name, origin);
        }
    };
    let outcome = target
        .map_err(|error| error.message)
        .and_then(|target| {
            collect::capture(&TargetRunner(target), &Options::default()).map_err(|e| e.message)
        })
        .and_then(|text| {
            store
                .put_with_origin(id, Slot::Base, &text, Some(&source), origin.as_deref())
                .map(|_| ())
        });
    if let Err(message) = outcome {
        let _ = store.put_failure(id, &message);
        keep_note(&source);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guest::test_support;

    #[test]
    fn a_failed_capture_is_recorded_and_the_note_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let context = test_support::RecordingContext::new(dir.path());
        let store = Store::for_engine(context.as_ref());
        // Nobody noted this computer: nothing happens.
        capture_with(
            context.as_ref(),
            Err(guest::GuestError::internal("x")),
            "plain",
            "id-0",
        );
        assert_eq!(store.get_failure("id-0"), None);

        store.put_pending("try", "cp1").unwrap();
        capture_with(
            context.as_ref(),
            Err(guest::GuestError::internal(
                "The computer is not reachable.",
            )),
            "try",
            "id-1",
        );
        assert_eq!(
            store.get_failure("id-1").as_deref(),
            Some("The computer is not reachable.")
        );
        assert_eq!(store.get("id-1", Slot::Base).unwrap(), None);
        assert_eq!(store.take_pending("try").as_deref(), Some("cp1"));
    }

    #[test]
    fn the_actual_checkpoint_replaces_the_noted_key() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let context = test_support::RecordingContext::new(dir.path());
        let store = Store::for_engine(context.as_ref());
        install(context.clone());
        // Noted with the source computer; the fork-point checkpoint then takes its place.
        expect(context.clone(), "try", "src", "src");
        forked_from("try", "cp-auto-1");
        assert_eq!(store.take_pending("try").as_deref(), Some("cp-auto-1"));
        // Nothing noted: nothing is created.
        forked_from("other", "cp-2");
        assert_eq!(store.take_pending("other"), None);
        expect(context.clone(), "gone", "src", "src");
        withdraw(context.as_ref(), "gone");
        assert_eq!(store.take_pending("gone"), None);
    }

    #[test]
    fn a_checkpoint_keys_the_baseline_and_the_current_state_keys_by_source() {
        assert_eq!(source_key(Some("cp1"), "src", None), "cp1");
        assert_eq!(source_key(None, "src", None), "src");
        assert_eq!(source_key(Some("cp1"), "src", Some("ab12")), "recipe:ab12");
    }
}
