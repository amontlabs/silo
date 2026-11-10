//! The logins found on a computer before it is shared: its disk, listed through the change
//! manifest capture, and its fork-time record. This only names them; an export of a Linux
//! virtual machine removes them from the exported computer through [`crate::export_scrub`],
//! which removes the files this lists.

use std::sync::Arc;

use serde::Serialize;
use silo_api::jobs::{ErrorCode, JobId};

use super::{
    collect::{self, Options, TargetRunner},
    ignore::login_label,
    keep::service,
    manifest::Manifest,
    store::{Slot, Store},
};
use crate::{
    checkpoints::Failure,
    guest::{self, GuestError},
    runtime,
};

/// What was looked at and what was found.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    /// The logins, by tool name and path, sorted and without repeats.
    pub logins: Vec<String>,
    /// `disk` and `fork-time record`: the places that were listed.
    pub checked: Vec<String>,
    /// Why Silo could not check the whole computer, when it could not (a stopped computer, a checkpoint).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unchecked: Option<String>,
    /// The export removes these logins from the computer it writes: it is made from a copy they
    /// are removed from. False when they stay in the file (a checkpoint, a computer that is not a
    /// Linux virtual machine) or when the person asked to include them.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub leaves_out: bool,
}

/// The login names among the file paths of a manifest.
pub fn logins_in(manifest_text: &str) -> Result<Vec<String>, String> {
    let manifest = Manifest::parse(manifest_text).map_err(|error| error.to_string())?;
    let mut logins: Vec<String> = manifest
        .files
        .keys()
        .filter_map(|path| login_label(path))
        .collect();
    logins.sort();
    logins.dedup();
    Ok(logins)
}

/// The paths of the files of a manifest that [`login_label`] names, sorted.
pub fn login_paths_in(manifest_text: &str) -> Result<Vec<String>, String> {
    let manifest = Manifest::parse(manifest_text).map_err(|error| error.to_string())?;
    Ok(manifest
        .files
        .keys()
        .filter(|path| login_label(path).is_some())
        .cloned()
        .collect())
}

fn merge(found: &mut Found, logins: Vec<String>) {
    found.logins.extend(logins);
    found.logins.sort();
    found.logins.dedup();
}

/// Lists the logins of the computer `id`. The disk is listed with one manifest capture (the
/// guest walks `/etc`, `/root`, `/home`, `/usr/local`, `/opt` and `/srv`, as a diff does) and
/// only while the computer runs, so a check never starts it; a checkpoint export packages
/// another disk than the live one, which is not listed either.
pub fn check(
    host: &Arc<dyn runtime::RuntimeHost>,
    id: &str,
    from_checkpoint: bool,
    include_logins: bool,
) -> Result<Found, String> {
    let context = host.engine_context();
    let store = Store::for_engine(context.as_ref());
    let mut found = Found {
        leaves_out: !from_checkpoint
            && !include_logins
            && crate::backup_controller::leaves_logins_out(host.as_ref(), id),
        ..Found::default()
    };
    if let Some((record, _)) = store.get(id, Slot::Base)? {
        merge(&mut found, logins_in(&record)?);
        found.checked.push("fork-time record".into());
    }
    if from_checkpoint {
        found.unchecked =
            Some("the files of a checkpoint cannot be listed, only its fork-time record".into());
        return Ok(found);
    }
    let target = guest::resolve(
        runtime::runtime_paths(host.as_ref()),
        &context.paths().app_data,
        id,
    )
    .map_err(|error| error.message)?;
    list_disk(&mut found, target.ensure_running(), || {
        let text = collect::capture(&TargetRunner(target), &Options::default())
            .map_err(|error| error.message)?;
        logins_in(&text)
    });
    Ok(found)
}

/// Adds the logins of the disk to `found`. A computer that cannot be listed leaves the logins
/// already found in place and says why, instead of discarding them with an error.
fn list_disk(
    found: &mut Found,
    running: Result<(), GuestError>,
    list: impl FnOnce() -> Result<Vec<String>, String>,
) {
    if let Err(error) = running {
        found.unchecked = Some(if error.code == ErrorCode::NotRunning {
            "the computer is stopped, so its files were not listed; start it to check them".into()
        } else {
            format!(
                "the computer could not be reached, so its files were not listed ({})",
                error.message.trim().trim_end_matches('.')
            )
        });
        return;
    }
    match list() {
        Ok(logins) => {
            merge(found, logins);
            found.checked.push("disk".into());
        }
        Err(why) => {
            found.unchecked = Some(format!(
                "its files could not be listed ({})",
                why.trim().trim_end_matches('.')
            ));
        }
    }
}

/// `exports.check`: a job whose result is [`Found`].
pub fn start_check(
    host: Arc<dyn runtime::RuntimeHost>,
    computer_id: String,
    from_checkpoint: bool,
    include_logins: bool,
) -> Result<JobId, Failure> {
    service::start(host, "exports.check", move |host| {
        let found = check(host, &computer_id, from_checkpoint, include_logins)?;
        serde_json::to_value(found).map_err(|error| error.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = "META\tformat\t2\n\
F\t/etc/shadow\t10\t1\taa\n\
F\t/etc/ssh/ssh_host_ed25519_key\t10\t1\tbb\n\
F\t/root/.claude/.credentials.json\t10\t1\tcc\n\
F\t/home/dev/.config/gh/hosts.yml\t10\t1\tdd\n\
F\t/home/dev/app/main.rs\t10\t1\tee\n";

    #[test]
    fn a_manifest_lists_logins_by_name_and_not_the_credentials_every_computer_has() {
        assert_eq!(
            logins_in(MANIFEST).unwrap(),
            vec![
                "a Claude Code login (/root/.claude/.credentials.json)".to_owned(),
                "a GitHub CLI login (/home/dev/.config/gh/hosts.yml)".to_owned(),
            ]
        );
    }

    #[test]
    fn an_unreadable_manifest_is_an_error_not_a_clean_bill() {
        assert!(logins_in("F\t/x\n").is_err());
    }

    fn with_record_login() -> Found {
        let mut found = Found::default();
        merge(
            &mut found,
            vec!["a GitHub CLI login (/home/dev/.config/gh/hosts.yml)".into()],
        );
        found.checked.push("fork-time record".into());
        found
    }

    #[test]
    fn a_disk_that_cannot_be_listed_keeps_the_logins_the_record_named() {
        let mut found = with_record_login();
        list_disk(&mut found, Ok(()), || Err("The guest timed out.".into()));
        assert_eq!(found.logins.len(), 1);
        assert_eq!(found.checked, ["fork-time record"]);
        assert_eq!(
            found.unchecked.as_deref(),
            Some("its files could not be listed (The guest timed out)")
        );
    }

    #[test]
    fn a_computer_that_is_not_stopped_but_cannot_be_reached_says_so() {
        let mut stopped = with_record_login();
        list_disk(
            &mut stopped,
            Err(GuestError::from("Start dev first.".to_owned())),
            || unreachable!(),
        );
        assert!(stopped
            .unchecked
            .unwrap()
            .starts_with("the computer is stopped"));

        let mut broken = with_record_login();
        list_disk(
            &mut broken,
            Err(GuestError::from("The runtime did not answer.".to_owned())),
            || unreachable!(),
        );
        let why = broken.unchecked.unwrap();
        assert!(!why.contains("stopped"), "{why}");
        assert!(why.contains("The runtime did not answer"), "{why}");
        assert_eq!(broken.logins.len(), 1);
    }

    #[test]
    fn a_listed_disk_adds_its_logins() {
        let mut found = with_record_login();
        list_disk(&mut found, Ok(()), || {
            Ok(vec![
                "a Claude Code login (/root/.claude/.credentials.json)".into(),
            ])
        });
        assert_eq!(found.logins.len(), 2);
        assert_eq!(found.checked, ["fork-time record", "disk"]);
        assert!(found.unchecked.is_none());
    }

    #[test]
    fn found_serializes_without_an_empty_reason() {
        let found = Found {
            logins: vec!["a".into()],
            checked: vec!["disk".into()],
            unchecked: None,
            leaves_out: false,
        };
        assert_eq!(
            serde_json::to_value(found).unwrap(),
            serde_json::json!({"logins": ["a"], "checked": ["disk"]})
        );
    }

    #[test]
    fn found_says_when_the_export_leaves_the_logins_out() {
        let found = Found {
            leaves_out: true,
            ..Found::default()
        };
        assert_eq!(
            serde_json::to_value(found).unwrap(),
            serde_json::json!({"logins": [], "checked": [], "leavesOut": true})
        );
    }

    #[test]
    fn the_paths_of_a_manifest_that_are_logins_are_the_ones_the_scan_names() {
        assert_eq!(
            login_paths_in(MANIFEST).unwrap(),
            vec![
                "/home/dev/.config/gh/hosts.yml".to_owned(),
                "/root/.claude/.credentials.json".to_owned(),
            ]
        );
        assert!(login_paths_in("F\t/x\n").is_err());
    }
}
