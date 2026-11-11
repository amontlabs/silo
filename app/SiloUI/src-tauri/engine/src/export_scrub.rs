//! Exporting a Linux computer without its login files, as far as the file system goes.
//!
//! The export is not made from the computer itself. A hidden, short-lived copy of it is forked
//! with no secrets, no GitHub access and a closed network, started, and stripped of the login files
//! the scan names (`changes::ignore::login_label` is the one list). The export captures that copy
//! under the original's name, and the copy is dropped whether the export worked or not.
//!
//! This keeps the files out of the imported computer; it does not keep their bytes out of the file.
//! A copy is a fork, and a fork's disks are layers over the computer's own: the layers the fork
//! shares with the computer (and the checkpoint it took) are read-only and are written into the
//! export whole, with the bytes of the deleted files in them. The canary check in
//! `docs/direction/checks/t14_export_scrub_check.sh` finds a marker written to a login path in the
//! exported layers after the file is deleted and the free blocks are discarded, so nothing here may
//! be described as keeping the bytes out.
//!
//! The computer-level steps (fork, start, run a command, stop, delete) are supplied by
//! [`ScrubEnv`]; `silod` implements it with the engine's own methods.

use crate::{changes::ignore::login_label, recipes::ExecOut};

/// Hidden copies are named `silo-export-<8 hex>`. The name keeps them out of every list and
/// reserves it against computers a person or an agent names.
pub const COPY_PREFIX: &str = "silo-export-";

/// A new name for a copy.
pub fn copy_name() -> String {
    let id = uuid::Uuid::new_v4().simple().to_string();
    format!("{COPY_PREFIX}{}", &id[..8])
}

/// Whether `name` is one a copy gets.
pub fn is_copy(name: &str) -> bool {
    name.strip_prefix(COPY_PREFIX)
        .is_some_and(|suffix| suffix.len() == 8 && suffix.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Lists, from the paths on stdin (NUL separated), the ones that still exist.
const SURVIVORS_SCRIPT: &str = "xargs -0 sh -c 'for p; do if [ -e \"$p\" ] || [ -L \"$p\" ]; then printf \"%s\\n\" \"$p\"; fi; done' sh";

/// Removes the paths on stdin (NUL separated).
const REMOVE_SCRIPT: &str = "xargs -0 rm -f --";

/// What the export needs from the engine to make, use and drop the copy.
pub trait ScrubEnv: Send + Sync {
    /// Every computer, as (id, name).
    fn computers(&self) -> Result<Vec<(String, String)>, String>;
    /// Forks `source_id` as `name` with no secrets, no GitHub access and a closed network.
    /// Returns the copy's id.
    fn fork(&self, source_id: &str, name: &str) -> Result<String, String>;
    fn start(&self, computer_id: &str) -> Result<(), String>;
    /// Runs `script` with `sh -c` as root, with `stdin` on its standard input.
    fn run(&self, computer_id: &str, script: &str, stdin: &[u8]) -> Result<ExecOut, String>;
    /// The paths of the files on a running computer that `login_label` names.
    fn login_paths(&self, computer_id: &str) -> Result<Vec<String>, String>;
    fn stop(&self, computer_id: &str) -> Result<(), String>;
    /// Stops the computer if it runs, then deletes it.
    fn delete(&self, computer_id: &str) -> Result<(), String>;
}

/// A copy that is ready to be exported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prepared {
    pub copy_id: String,
    pub copy_name: String,
    /// The computer the copy was made from.
    pub source_id: String,
    /// What the copy no longer holds, by the name an export warning shows.
    pub left_out: Vec<String>,
}

/// Why no copy was prepared.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    Cancelled,
    Failed(String),
}

impl Failure {
    fn failed(message: impl Into<String>) -> Self {
        Failure::Failed(message.into())
    }

    fn with_cleanup(self, cleanup: Result<(), String>) -> Self {
        match (self, cleanup) {
            (Failure::Failed(message), Err(note)) => Failure::Failed(format!("{message} {note}")),
            (other, _) => other,
        }
    }
}

/// Drops the copies a failed run left behind. A copy exists only while an export runs, and
/// exports run one at a time, so every copy found before an export makes its own is stale.
pub fn drop_stale(env: &dyn ScrubEnv) {
    for (id, name) in env.computers().unwrap_or_default() {
        if is_copy(&name) {
            let _ = env.delete(&id);
        }
    }
}

/// Makes the copy of `source_id` and strips it. The caller drops stale copies first
/// ([`drop_stale`]), once per export, so a second computer's copy never drops the first's. `progress` gets a plain sentence for each step.
/// On any failure the copy is dropped before this returns.
pub fn prepare(
    env: &dyn ScrubEnv,
    source_id: &str,
    progress: &dyn Fn(&str),
    cancelled: &dyn Fn() -> bool,
) -> Result<Prepared, Failure> {
    if cancelled() {
        return Err(Failure::Cancelled);
    }
    let name = copy_name();
    progress("Making a temporary copy of the computer");
    let copy_id = match env.fork(source_id, &name) {
        Ok(id) => id,
        Err(error) => {
            // The fork may have left a half-made copy.
            let found = env
                .computers()
                .unwrap_or_default()
                .into_iter()
                .find(|(_, existing)| *existing == name);
            let cleanup = found.map(|(id, _)| env.delete(&id)).unwrap_or(Ok(()));
            return Err(Failure::failed(format!(
                "Silo could not make the temporary copy: {error}."
            ))
            .with_cleanup(cleanup));
        }
    };
    // The copy has no use for Silo's tools: the engine writes no endpoint file into it and does
    // not reconcile it.
    if let Err(error) = crate::guest_client::turn_off_copy(&copy_id) {
        let cleanup = env.delete(&copy_id);
        return Err(Failure::failed(format!(
            "Silo could not turn off Silo's tools in the temporary copy: {error}."
        ))
        .with_cleanup(cleanup));
    }
    let mut prepared = Prepared {
        copy_id,
        copy_name: name,
        source_id: source_id.to_owned(),
        left_out: Vec::new(),
    };
    match strip(env, &mut prepared, progress, cancelled) {
        Ok(()) => Ok(prepared),
        Err(failure) => {
            let cleanup = drop_copy(env, &prepared);
            Err(failure.with_cleanup(cleanup))
        }
    }
}

fn strip(
    env: &dyn ScrubEnv,
    prepared: &mut Prepared,
    progress: &dyn Fn(&str),
    cancelled: &dyn Fn() -> bool,
) -> Result<(), Failure> {
    let id = prepared.copy_id.clone();
    let step = |what: &str, result: Result<(), String>| {
        result.map_err(|error| Failure::failed(format!("{what}: {error}.")))
    };
    let proceed = || if cancelled() { Err(Failure::Cancelled) } else { Ok(()) };
    proceed()?;
    progress("Starting the copy");
    step("Silo could not start the temporary copy", env.start(&id))?;
    proceed()?;
    progress("Looking for logins in the copy");
    let paths = env.login_paths(&id).map_err(|error| {
        Failure::failed(format!("Silo could not look for logins in the copy: {error}."))
    })?;
    progress("Removing the logins from the copy");
    step(
        "Silo could not remove the logins",
        remove(env, &id, &with_silo_files(&paths)),
    )?;
    prepared.left_out = paths
        .iter()
        .filter_map(|path| login_label(path))
        .collect::<Vec<_>>();
    prepared.left_out.sort();
    prepared.left_out.dedup();
    proceed()?;
    progress("Stopping the copy");
    step("Silo could not stop the temporary copy", env.stop(&id))
}

/// `paths` and the files of Silo's tools in a computer: the endpoint file with its token, and
/// the earlier client's token file.
fn with_silo_files(paths: &[String]) -> Vec<String> {
    use silo_api::methods::guest_client::{GUEST_ENDPOINT_PATH, LEGACY_GUEST_TOKEN_PATH};
    let mut all = paths.to_vec();
    for path in [GUEST_ENDPOINT_PATH, LEGACY_GUEST_TOKEN_PATH] {
        if !all.iter().any(|existing| existing == path) {
            all.push(path.to_owned());
        }
    }
    all
}

fn nul_separated(paths: &[String]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for path in paths {
        bytes.extend_from_slice(path.as_bytes());
        bytes.push(0);
    }
    bytes
}

/// Removes `paths` and checks that none is left.
fn remove(env: &dyn ScrubEnv, id: &str, paths: &[String]) -> Result<(), String> {
    if let Some(path) = paths
        .iter()
        .find(|path| !path.starts_with('/') || path.contains('\0'))
    {
        return Err(format!("{path:?} is not an absolute path"));
    }
    let input = nul_separated(paths);
    let removed = env.run(id, REMOVE_SCRIPT, &input)?;
    if removed.code != 0 {
        return Err(format!(
            "the removal ended with code {}: {}",
            removed.code,
            tail(&removed)
        ));
    }
    let left = env.run(id, SURVIVORS_SCRIPT, &input)?;
    let survivors = String::from_utf8_lossy(&left.stdout);
    let survivors = survivors.trim();
    if left.code != 0 || !survivors.is_empty() {
        let shown = if survivors.is_empty() {
            tail(&left)
        } else {
            survivors.lines().take(5).collect::<Vec<_>>().join(", ")
        };
        return Err(format!("these files are still there: {shown}"));
    }
    Ok(())
}

/// The last words a command said, on stderr first.
fn tail(out: &ExecOut) -> String {
    let text = if out.stderr.is_empty() {
        &out.stdout
    } else {
        &out.stderr
    };
    let text = String::from_utf8_lossy(text);
    let text = text.trim();
    let skip = text.chars().count().saturating_sub(300);
    text.chars().skip(skip).collect()
}

/// Drops the copy. The disk-only checkpoint the fork took of the source stays, as it does for any
/// fork: the next checkpoint and export of the source build on it.
pub fn drop_copy(env: &dyn ScrubEnv, prepared: &Prepared) -> Result<(), String> {
    env.delete(&prepared.copy_id).map_err(|error| {
        format!(
            "Silo could not drop the temporary copy {} ({error}).",
            prepared.copy_name
        )
    })
}

#[cfg(test)]
mod tests;
