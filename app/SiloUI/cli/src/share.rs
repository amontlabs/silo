//! `silo export` and `silo import`: a whole computer as a file, on this device.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use silo_api::methods::{
    exports::{ExportCheckParams, ExportCreateParams, ExportImportParams},
    requests,
};

use crate::{
    engine::{Computer, Engine, Progress},
    error::CliError,
    ops,
};

/// The file extension of an export.
pub const EXTENSION: &str = "silo-backup";

/// Where the answer to "Share anyway?" comes from.
pub trait Asker {
    /// Whether a person can be asked.
    fn interactive(&self) -> bool;
    /// Shows `prompt` and says whether the answer was yes.
    fn ask(&self, prompt: &str) -> Result<bool, CliError>;
}

/// The terminal: asks on stderr, reads one line from stdin, and only a `y` or `yes` is a yes.
pub struct Terminal;

impl Asker for Terminal {
    fn interactive(&self) -> bool {
        use std::io::IsTerminal;
        std::io::stdin().is_terminal()
    }

    fn ask(&self, prompt: &str) -> Result<bool, CliError> {
        use std::io::Write;
        eprint!("{prompt} ");
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        std::io::stdin()
            .read_line(&mut line)
            .map_err(|error| CliError::new(format!("Cannot read the answer: {error}")))?;
        Ok(matches!(
            line.trim().to_ascii_lowercase().as_str(),
            "y" | "yes"
        ))
    }
}

#[derive(Clone, Debug, Default)]
pub struct ExportArgs {
    pub computer: String,
    pub output: Option<PathBuf>,
    /// A checkpoint of the computer, by id or name.
    pub checkpoint: Option<String>,
    /// Share without asking when logins are found or the check could not run.
    pub yes: bool,
}

/// `<computer>-<YYYY-MM-DD>.silo-backup`, with the first free `-2`, `-3`, ... in `folder`.
pub fn default_archive(folder: &Path, computer: &str, unix: i64) -> PathBuf {
    let date = &crate::clock::format_rfc3339_utc(unix)[..10];
    let first = folder.join(format!("{computer}-{date}.{EXTENSION}"));
    if !first.exists() {
        return first;
    }
    (2_u32..)
        .map(|n| folder.join(format!("{computer}-{date}-{n}.{EXTENSION}")))
        .find(|path| !path.exists())
        .unwrap_or(first)
}

fn absolute(path: &Path, working_directory: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        working_directory.join(path)
    }
}

fn checkpoint_id(computer: &Computer, wanted: &str) -> Result<String, CliError> {
    let matches: Vec<&Value> = computer
        .checkpoints
        .iter()
        .filter(|checkpoint| checkpoint["id"] == wanted || checkpoint["name"] == wanted)
        .collect();
    match matches.as_slice() {
        [one] => Ok(one["id"].as_str().unwrap_or(wanted).to_owned()),
        [] => Err(CliError::new(format!(
            "{} has no checkpoint {wanted}. See `silo checkpoints {}`.",
            computer.name, computer.name
        ))),
        _ => Err(CliError::new(format!(
            "More than one checkpoint of {} is called {wanted}; use its id.",
            computer.name
        ))),
    }
}

/// The sentence that names the logins found: `a Claude Code login (...), a GitHub CLI login (...)`.
fn names(logins: &[String]) -> String {
    logins.join(", ")
}

/// Exports `args.computer` to a file. Logins on the computer, or that Silo could not check for
/// them, are named first; the files stay in the export either way.
pub async fn export(
    engine: &Engine,
    args: &ExportArgs,
    working_directory: &Path,
    asker: &dyn Asker,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let target = engine.computer(&args.computer).await?;
    let checkpoint = args
        .checkpoint
        .as_deref()
        .map(|wanted| checkpoint_id(&target, wanted))
        .transpose()?;
    let found = engine
        .run_job(
            &format!("Looking for logins on {}", target.name),
            requests::exports_check(ExportCheckParams {
                computer_id: target.id.clone(),
                checkpoint_id: checkpoint.clone(),
                device_id: None,
            }),
            progress,
        )
        .await;
    // A check that did not run, could only look at part of the computer, or failed is one case:
    // Silo could not check, and `unchecked` says why.
    let (logins, unchecked): (Vec<String>, Option<String>) = match &found {
        Ok(found) => (
            found["logins"]
                .as_array()
                .map(|logins| {
                    logins
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            found["unchecked"].as_str().map(str::to_owned),
        ),
        Err(error) => (Vec::new(), Some(error.message.clone())),
    };
    let mut concerns = Vec::new();
    if !logins.is_empty() {
        concerns.push(format!("This computer contains {}", names(&logins)));
    }
    if let Some(why) = &unchecked {
        concerns.push(format!(
            "Silo could not check {} for {}logins ({})",
            target.name,
            if logins.is_empty() { "" } else { "other " },
            why.trim().trim_end_matches('.')
        ));
    }
    if !concerns.is_empty() && !args.yes {
        let concern = concerns.join(". ");
        if !asker.interactive() {
            return Err(CliError::new(format!(
                "{concern}. Pass --yes to share it anyway; the files stay in the export."
            )));
        }
        if !asker.ask(&format!("{concern}. Share anyway? [y/N]"))? {
            return Err(CliError::new(format!(
                "Not shared: {} was not exported.",
                target.name
            )));
        }
    }
    let archive = match &args.output {
        Some(path) => absolute(path, working_directory),
        None => default_archive(working_directory, &target.name, crate::clock::now_unix()),
    };
    let folder = archive
        .parent()
        .ok_or_else(|| CliError::new("The export file needs a folder."))?
        .to_string_lossy()
        .into_owned();
    let result = engine
        .run_job(
            &format!("Exporting {}", target.name),
            requests::exports_create(ExportCreateParams {
                destination: folder,
                computer_ids: vec![target.id.clone()],
                checkpoint_id: checkpoint.clone(),
                archive_path: Some(archive.to_string_lossy().into_owned()),
                device_id: None,
            }),
            progress,
        )
        .await?;
    transfer_outcome(&result, "Export")?;
    Ok(json!({
        "computer": target.name,
        "archive": result["archive"]["archivePath"],
        "size": result["archive"]["size"],
        "checkpoint": result["archive"]["checkpointName"],
        "logins": logins,
        "unchecked": unchecked,
    }))
}

/// An export or import that ended without success is an error carrying the engine's reason.
fn transfer_outcome(result: &Value, what: &str) -> Result<(), CliError> {
    if result["outcome"] == "success" {
        return Ok(());
    }
    let message = result["message"].as_str().unwrap_or("it did not finish");
    Err(CliError::new(match result["detail"].as_str() {
        Some(detail) => format!("{what} failed: {message} {detail}"),
        None => format!("{what} failed: {message}"),
    }))
}

/// `main-2026-11-07` and `main-2026-11-07-2` are `main`.
pub fn computer_of_archive(file: &Path) -> String {
    let stem = file
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("computer");
    let stem = stem.strip_suffix(&format!(".{EXTENSION}")).unwrap_or(stem);
    let mut parts: Vec<&str> = stem.split('-').collect();
    let numbered = |part: &str| part.len() <= 2 && part.chars().all(|c| c.is_ascii_digit());
    let is_date = |parts: &[&str]| {
        parts.len() >= 3
            && parts[parts.len() - 3].len() == 4
            && parts[parts.len() - 3..]
                .iter()
                .all(|part| part.chars().all(|c| c.is_ascii_digit()))
    };
    if parts.len() > 4 && numbered(parts[parts.len() - 1]) && is_date(&parts[..parts.len() - 1]) {
        parts.pop();
    }
    if is_date(&parts) {
        parts.truncate(parts.len() - 3);
    }
    let name = parts.join("-");
    if name.is_empty() {
        "computer".into()
    } else {
        name
    }
}

/// The name an imported computer gets when none is given: the sender's, or that and
/// `-imported` (`-imported-2`, ...) when this device already has a computer by that name.
pub fn default_import_name(file: &Path, taken: &[String]) -> String {
    let base = computer_of_archive(file);
    if !taken.iter().any(|name| name.eq_ignore_ascii_case(&base)) {
        return base;
    }
    let first = format!("{base}-imported");
    if !taken.iter().any(|name| name.eq_ignore_ascii_case(&first)) {
        return first;
    }
    (2_u32..)
        .map(|n| format!("{first}-{n}"))
        .find(|name| !taken.iter().any(|taken| taken.eq_ignore_ascii_case(name)))
        .unwrap_or(first)
}

/// Imports the computer in `file` as a new, stopped computer.
pub async fn import(
    engine: &Engine,
    file: &Path,
    name: Option<&str>,
    working_directory: &Path,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let path = absolute(file, working_directory);
    if !path.is_file() {
        return Err(CliError::new(format!(
            "{} is not a file. Name the .{EXTENSION} file to import.",
            path.display()
        )));
    }
    let taken: Vec<String> = engine
        .all_computers()
        .await?
        .into_iter()
        .map(|computer| computer.name)
        .collect();
    let new_name = match name {
        Some(name) => name.to_owned(),
        None => default_import_name(&path, &taken),
    };
    let result = engine
        .run_job(
            &format!("Importing {new_name}"),
            requests::exports_import(ExportImportParams {
                archive_path: path.to_string_lossy().into_owned(),
                new_name: new_name.clone(),
                source_name: None,
                device_id: None,
            }),
            progress,
        )
        .await?;
    transfer_outcome(&result, "Import")?;
    let mut imported = ops::status(engine, &new_name).await?;
    imported["archive"] = json!(path.to_string_lossy());
    Ok(imported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_export_file_is_named_for_the_computer_and_the_day_and_never_replaces_one() {
        let folder = tempfile::tempdir().unwrap();
        let first = default_archive(folder.path(), "main", 1_794_000_000);
        assert_eq!(first.file_name().unwrap(), "main-2026-11-06.silo-backup");
        std::fs::write(&first, "x").unwrap();
        let second = default_archive(folder.path(), "main", 1_794_000_000);
        assert_eq!(second.file_name().unwrap(), "main-2026-11-06-2.silo-backup");
    }

    #[test]
    fn the_sender_s_name_comes_back_out_of_the_file_name() {
        for (file, name) in [
            ("/x/main-2026-11-07.silo-backup", "main"),
            ("/x/main-2026-11-07-2.silo-backup", "main"),
            ("/x/my-app-2026-11-07.silo-backup", "my-app"),
            ("/x/friend.silo-backup", "friend"),
            ("/x/2026-11-07.silo-backup", "computer"),
        ] {
            assert_eq!(computer_of_archive(Path::new(file)), name, "{file}");
        }
    }

    #[test]
    fn an_imported_computer_never_takes_a_name_that_is_in_use() {
        let file = Path::new("main-2026-11-07.silo-backup");
        let taken = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
        assert_eq!(default_import_name(file, &taken(&[])), "main");
        assert_eq!(
            default_import_name(file, &taken(&["Main"])),
            "main-imported"
        );
        assert_eq!(
            default_import_name(file, &taken(&["main", "main-imported"])),
            "main-imported-2"
        );
    }
}
