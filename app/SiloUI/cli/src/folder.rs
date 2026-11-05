//! `silo fork ./dir`, and folding a folder fork back: the host folder's git repository goes
//! into a fresh Linux fork as a bundle, and only commits come back, as a bundle fetched into
//! `refs/silo/<fork>/head`. Everything done on the host stays inside that repository.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use silo_api::methods::{
    files::{ConflictPolicy, FileDownloadParams, FileUploadParams},
    requests,
};

use crate::{
    config::{Recipe, Settings},
    engine::{Computer, Engine, Progress},
    error::CliError,
    ops::{self, ForkArgs},
};

/// The CLI memory file that records which forks hold a host folder.
const MEMORY_FILE: &str = "cli-folders.json";
/// A folder of the repository's git directory that holds bundles in transit.
const TRANSIT: &str = "silo";

/// Whether `source` names a folder rather than a computer: computer names have no `/`.
pub fn is_folder(source: &str) -> bool {
    source == "." || source == ".." || source.contains('/') || source.starts_with('~')
}

// MARK: Records

/// What is remembered about a fork made from a host folder.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub fork_id: String,
    pub fork: String,
    /// The host folder.
    pub path: String,
    /// The repository inside the fork.
    pub guest_path: String,
    /// The host commit the fork started from.
    pub base: String,
    pub branch: Option<String>,
    /// The latest fold, which `silo undo` puts back.
    pub backup: Option<Backup>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    #[serde(rename = "ref")]
    pub reference: String,
    pub branch: String,
    /// The host HEAD before the fold.
    pub before: String,
    /// The host HEAD the fold left.
    pub after: String,
}

pub fn record_for(engine: &Engine, fork_id: &str) -> Option<Record> {
    engine
        .recall(MEMORY_FILE, fork_id)
        .and_then(|value| serde_json::from_value(value).ok())
}

fn save(engine: &Engine, record: &Record) -> Result<(), CliError> {
    engine.remember(
        MEMORY_FILE,
        &record.fork_id,
        serde_json::to_value(record).unwrap_or_default(),
    )
}

/// The record of the fork that holds `path`, the most recently folded first.
fn record_for_path(engine: &Engine, path: &Path) -> Option<Record> {
    let wanted = path.to_string_lossy();
    engine
        .recall_all(MEMORY_FILE)
        .into_values()
        .filter_map(|value| serde_json::from_value::<Record>(value).ok())
        .filter(|record| record.path == wanted && record.backup.is_some())
        .max_by_key(|record| record.backup.as_ref().map(|b| b.reference.clone()))
}

// MARK: Git on the host

struct Git {
    dir: PathBuf,
}

struct GitOutput {
    code: i32,
    out: String,
    err: String,
}

impl Git {
    async fn run(&self, args: &[&str]) -> Result<GitOutput, CliError> {
        let dir = self.dir.clone();
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        tokio::task::spawn_blocking(move || {
            let mut command = std::process::Command::new("git");
            command
                .arg("-C")
                .arg(&dir)
                .args(&args)
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_INDEX_FILE")
                .env("GIT_TERMINAL_PROMPT", "0");
            command.output().map(|output| GitOutput {
                code: output.status.code().unwrap_or(-1),
                out: String::from_utf8_lossy(&output.stdout)
                    .trim_end()
                    .to_owned(),
                err: String::from_utf8_lossy(&output.stderr)
                    .trim_end()
                    .to_owned(),
            })
        })
        .await
        .map_err(|_| CliError::new("git stopped unexpectedly."))?
        .map_err(|error| CliError::new(format!("Cannot run git: {error}")))
    }

    /// The output of a git command that must succeed.
    async fn ok(&self, args: &[&str]) -> Result<String, CliError> {
        let output = self.run(args).await?;
        if output.code == 0 {
            Ok(output.out)
        } else {
            Err(CliError::new(format!(
                "git {} failed: {}",
                args.first().copied().unwrap_or_default(),
                if output.err.is_empty() {
                    output.out
                } else {
                    output.err
                }
            )))
        }
    }

    async fn succeeds(&self, args: &[&str]) -> Result<bool, CliError> {
        Ok(self.run(args).await?.code == 0)
    }
}

/// A repository's top folder, its HEAD commit and its branch.
struct Repo {
    git: Git,
    head: String,
    branch: Option<String>,
    git_dir: PathBuf,
}

impl Repo {
    async fn open(dir: &Path) -> Result<Self, CliError> {
        let dir = std::fs::canonicalize(dir)
            .map_err(|_| CliError::new(format!("{} is not a folder.", dir.display())))?;
        let git = Git { dir: dir.clone() };
        let top = git.run(&["rev-parse", "--show-toplevel"]).await?;
        if top.code != 0 {
            return Err(CliError::new(format!(
                "{} is not a git repository. Run `git init` and commit first; only git changes come back.",
                dir.display()
            )));
        }
        let top = std::fs::canonicalize(&top.out).unwrap_or_else(|_| PathBuf::from(&top.out));
        if top != dir {
            return Err(CliError::new(format!(
                "{} is inside the repository at {}. Give that folder, the top of the repository.",
                dir.display(),
                top.display()
            )));
        }
        let head = git.run(&["rev-parse", "--verify", "-q", "HEAD"]).await?;
        if head.code != 0 {
            return Err(CliError::new(format!(
                "{} has no commits yet; commit something first.",
                dir.display()
            )));
        }
        let branch = git.run(&["symbolic-ref", "--short", "-q", "HEAD"]).await?;
        let git_dir = git.ok(&["rev-parse", "--absolute-git-dir"]).await?;
        Ok(Self {
            head: head.out,
            branch: (branch.code == 0 && !branch.out.is_empty()).then_some(branch.out),
            git_dir: PathBuf::from(git_dir),
            git,
        })
    }

    /// Files a bundle can be written to or fetched from, inside the repository.
    fn transit(&self, name: &str) -> Result<PathBuf, CliError> {
        let directory = self.git_dir.join(TRANSIT);
        std::fs::create_dir_all(&directory).map_err(|error| {
            CliError::new(format!("Cannot create {}: {error}", directory.display()))
        })?;
        Ok(directory.join(name))
    }

    async fn dirty(&self) -> Result<bool, CliError> {
        Ok(!self
            .git
            .ok(&["status", "--porcelain", "--untracked-files=no"])
            .await?
            .is_empty())
    }
}

fn clean_up(path: &Path) {
    let _ = std::fs::remove_file(path);
    if let Some(parent) = path.parent() {
        // Only an empty transit folder goes.
        let _ = std::fs::remove_dir(parent);
    }
}

// MARK: Guest

fn exec(computer: &str, script: &str, args: &[&str], cwd: Option<&str>) -> ops::ExecArgs {
    let mut argv = vec!["sh".to_owned(), "-c".into(), script.to_owned(), "sh".into()];
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    ops::ExecArgs {
        computer: computer.to_owned(),
        argv,
        cwd: cwd.map(str::to_owned),
        timeout_secs: 120,
    }
}

async fn guest(
    engine: &Engine,
    computer: &str,
    script: &str,
    args: &[&str],
    what: &str,
) -> Result<String, CliError> {
    let output = ops::exec_raw(engine, &exec(computer, script, args, None)).await?;
    if output.process_exit_code() != 0 {
        let message = String::from_utf8_lossy(&output.stderr);
        return Err(CliError::new(format!(
            "{what} failed in {computer}: {}",
            message.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn transfer_id() -> String {
    format!("silo-{}", uuid::Uuid::new_v4().simple())
}

async fn upload(
    engine: &Engine,
    computer: &Computer,
    directory: &str,
    file: &Path,
    progress: Progress<'_>,
) -> Result<(), CliError> {
    let outcome = engine
        .run_job(
            &format!("Sending the repository to {}", computer.name),
            requests::files_upload(FileUploadParams {
                transfer_id: transfer_id(),
                computer_id: computer.id.clone(),
                device_id: None,
                directory: directory.to_owned(),
                conflict: ConflictPolicy::Replace,
                selection: None,
                paths: Some(vec![file.to_string_lossy().into_owned()]),
                origin: None,
                connection_id: None,
            }),
            progress,
        )
        .await?;
    match outcome["status"].as_str() {
        Some("done") | None => Ok(()),
        Some(other) => Err(CliError::new(format!(
            "The upload to {} ended as {other}.",
            computer.name
        ))),
    }
}

async fn download(
    engine: &Engine,
    computer: &Computer,
    path: &str,
    destination: &Path,
    progress: Progress<'_>,
) -> Result<(), CliError> {
    let outcome = engine
        .run_job(
            &format!("Fetching the commits from {}", computer.name),
            requests::files_download(FileDownloadParams {
                transfer_id: transfer_id(),
                computer_id: computer.id.clone(),
                device_id: None,
                path: path.to_owned(),
                destination: Some(destination.to_string_lossy().into_owned()),
            }),
            progress,
        )
        .await?;
    match outcome["status"].as_str() {
        Some("done") | None if destination.is_file() => Ok(()),
        _ => Err(CliError::new(format!(
            "The download from {} did not finish.",
            computer.name
        ))),
    }
}

// MARK: Fork

fn valid_label(text: &str) -> String {
    let label: String = text
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let label = label.trim_matches('-').to_owned();
    if label.is_empty() {
        "folder".to_owned()
    } else {
        label
    }
}

/// Copies a host folder's git repository into a new Linux fork of the `linux-dev` recipe and
/// starts it. Only commits are copied; uncommitted changes stay on the host.
pub async fn fork_folder(
    engine: &Engine,
    dir: &Path,
    name: Option<&str>,
    recipe: &Recipe,
    settings: Option<&Settings>,
    secrets: &[String],
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let repo = Repo::open(dir).await?;
    let host_path = repo.git.dir.to_string_lossy().into_owned();
    let folder_name = repo
        .git
        .dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let label = valid_label(&folder_name);
    let name = name
        .map(str::to_owned)
        .unwrap_or_else(|| ops::default_fork_name(&label, &ops::short_id()));
    if engine.computer(&name).await.is_ok() {
        return Err(CliError::new(format!(
            "A computer called {name} already exists; pick another --name."
        )));
    }
    if repo.dirty().await? {
        progress("Uncommitted changes in the folder are not copied; only commits are.");
    }
    let bundle = repo.transit(&format!("{}.bundle", transfer_id()))?;
    let bundle_arg = bundle.to_string_lossy().into_owned();
    let made = repo
        .git
        .ok(&["bundle", "create", "--quiet", &bundle_arg, "--all"])
        .await;
    if let Err(error) = made {
        clean_up(&bundle);
        return Err(error);
    }
    let result = send(
        engine, &bundle, &name, &label, recipe, settings, secrets, progress,
    )
    .await;
    clean_up(&bundle);
    let (fork, guest_path) = result?;
    let record = Record {
        fork_id: fork.id.clone(),
        fork: fork.name.clone(),
        path: host_path.clone(),
        guest_path: guest_path.clone(),
        base: repo.head.clone(),
        branch: repo.branch.clone(),
        backup: None,
    };
    save(engine, &record)?;
    let mut value = fork.to_json();
    value["folder"] = json!(host_path);
    value["guestPath"] = json!(guest_path);
    value["urls"] = json!(crate::fanout::urls(engine, &fork).await);
    Ok(value)
}

#[allow(clippy::too_many_arguments)]
async fn send(
    engine: &Engine,
    bundle: &Path,
    name: &str,
    label: &str,
    recipe: &Recipe,
    settings: Option<&Settings>,
    secrets: &[String],
    progress: Progress<'_>,
) -> Result<(Computer, String), CliError> {
    ops::fork(
        engine,
        &ForkArgs {
            source: String::new(),
            name: Some(name.to_owned()),
            checkpoint: None,
            secrets: secrets.to_vec(),
            recipe: Some(recipe.clone()),
            recipe_name: Some(crate::config::DEFAULT_RECIPE.to_owned()),
        },
        settings,
        progress,
    )
    .await?;
    ops::start(engine, name, progress).await?;
    let fork = engine.computer(name).await?;
    let home = guest(
        engine,
        name,
        "printf %s \"$HOME\"",
        &[],
        "Reading the home folder",
    )
    .await?;
    if !home.starts_with('/') {
        return Err(CliError::new(format!(
            "{name} reported {home:?} as its home folder."
        )));
    }
    let inbox = format!("{home}/.silo-inbox");
    let guest_path = format!("{home}/{label}");
    guest(
        engine,
        name,
        "mkdir -p \"$1\"",
        &[&inbox],
        "Preparing the transfer",
    )
    .await?;
    upload(engine, &fork, &inbox, bundle, progress).await?;
    let file = bundle
        .file_name()
        .map(|file| file.to_string_lossy().into_owned())
        .unwrap_or_default();
    let remote = format!("{inbox}/{file}");
    // The clone's only remote is the bundle, which is removed with the inbox.
    let setup = "set -e; git clone -q \"$1\" \"$2\"; cd \"$2\"; git remote remove origin; \
                 git config user.name >/dev/null 2>&1 || git config user.name silo-fork; \
                 git config user.email >/dev/null 2>&1 || git config user.email silo-fork@localhost; \
                 rm -rf \"$3\"";
    guest(
        engine,
        name,
        setup,
        &[&remote, &guest_path, &inbox],
        "Cloning the repository",
    )
    .await?;
    Ok((fork, guest_path))
}

// MARK: Fold

fn refused(fork: &str, target: &str, reason: &str, message: String, hint: &str) -> Value {
    json!({
        "status": "refused",
        "report": {"fork": fork, "target": target, "summary": [], "steps": [], "notes": [], "checkpoint": null},
        "refusal": {"reason": reason, "message": message, "conflicts": [], "hint": hint},
    })
}

fn ref_label(fork: &str) -> String {
    fork.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn utc_stamp() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

/// Brings the commits of a folder fork back into its host folder: a fast-forward when the
/// folder has not moved on, else a merge when that is clean. The folder's tree must be clean
/// and nothing is changed when the commits conflict. A backup ref of the folder's HEAD is
/// made first; `undo` goes back to it. A dry run fetches the commits into
/// `refs/silo/<fork>/head` and changes nothing else.
pub async fn fold_folder(
    engine: &Engine,
    fork: &Computer,
    mut record: Record,
    dry_run: bool,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let target = record.path.clone();
    let repo = Repo::open(Path::new(&record.path)).await.map_err(|error| {
        CliError::new(format!(
            "The folder {} that {} was made from cannot be used: {}",
            record.path, fork.name, error.message
        ))
    })?;
    let Some(branch) = repo.branch.clone() else {
        return Ok(refused(
            &fork.name,
            &target,
            "git",
            format!("{target} has no branch checked out (a detached HEAD)."),
            "Check out a branch there, then fold again.",
        ));
    };
    if repo.dirty().await? {
        return Ok(refused(
            &fork.name,
            &target,
            "dirty",
            format!("{target} has uncommitted changes to tracked files."),
            "Commit or stash them, then fold again.",
        ));
    }
    if fork.state != "running" {
        return Err(CliError::new(format!(
            "{} is {}; start it with `silo start {}` so its commits can be read.",
            fork.name, fork.state, fork.name
        )));
    }
    // Uncommitted work in the fork is committed first, so the fork's files are not left out.
    let gather = "set -e; cd \"$1\"; git add -A; \
                  git diff --cached --quiet || git commit -q -m \"silo: uncommitted changes in $3\"; \
                  head=$(git rev-parse HEAD); \
                  if [ \"$head\" = \"$2\" ]; then echo unchanged; else \
                  git bundle create --quiet \"$1/.git/silo-out.bundle\" \"$2..HEAD\"; echo \"$head\"; fi";
    let produced = guest(
        engine,
        &fork.name,
        gather,
        &[&record.guest_path, &record.base, &fork.name],
        "Collecting the commits",
    )
    .await?;
    if produced == "unchanged" {
        return Ok(json!({
            "status": "unchanged",
            "report": {"fork": fork.name, "target": target, "summary": [],
                       "steps": ["The fork has no commits beyond the folder's; nothing to bring back."],
                       "notes": [], "checkpoint": null},
        }));
    }
    let remote_bundle = format!("{}/.git/silo-out.bundle", record.guest_path);
    let bundle = repo.transit(&format!("{}.bundle", transfer_id()))?;
    let fetched = fetch_into(
        engine,
        fork,
        &repo,
        &remote_bundle,
        &bundle,
        &ref_label(&fork.name),
        progress,
    )
    .await;
    clean_up(&bundle);
    let reference = fetched?;
    let _ = guest(
        engine,
        &fork.name,
        "rm -f \"$1\"",
        &[&remote_bundle],
        "Cleaning up",
    )
    .await;

    let count = repo
        .git
        .ok(&[
            "rev-list",
            "--count",
            &format!("{}..{reference}", repo.head),
        ])
        .await?;
    let log = repo
        .git
        .ok(&[
            "log",
            "--oneline",
            "--max-count=20",
            &format!("{}..{reference}", repo.head),
        ])
        .await?;
    let mut summary: Vec<String> = vec![format!(
        "{count} commit{} from {}",
        if count == "1" { "" } else { "s" },
        fork.name
    )];
    summary.extend(log.lines().map(|line| format!("  {line}")));
    let already = repo
        .git
        .succeeds(&["merge-base", "--is-ancestor", &reference, "HEAD"])
        .await?;
    if already {
        return Ok(json!({
            "status": "unchanged",
            "report": {"fork": fork.name, "target": target, "summary": [],
                       "steps": [format!("{target} already has these commits.")], "notes": [], "checkpoint": null},
        }));
    }
    let fast_forward = repo
        .git
        .succeeds(&["merge-base", "--is-ancestor", "HEAD", &reference])
        .await?;
    if !fast_forward {
        if let Some(conflicts) = conflicts_with(&repo, &reference).await? {
            let mut value = refused(
                &fork.name,
                &target,
                "divergent",
                format!("The fork's commits conflict with changes in {target}; nothing was changed."),
                &format!("Merge {reference} in {target} yourself, or redo the change on top of the folder's latest commit."),
            );
            value["refusal"]["conflicts"] = json!(conflicts
                .iter()
                .map(|file| json!({"kind": "git", "item": file, "detail": "changed on both sides"}))
                .collect::<Vec<_>>());
            return Ok(value);
        }
    }
    summary.push(if fast_forward {
        format!("fast-forward {branch}")
    } else {
        format!("merge into {branch}")
    });
    if dry_run {
        return Ok(json!({
            "status": "dry-run",
            "report": {"fork": fork.name, "target": target, "summary": summary,
                       "steps": [], "notes": [format!("The commits are fetched as {reference}.")],
                       "checkpoint": null},
        }));
    }
    let backup = unique_backup(&repo).await?;
    repo.git.ok(&["update-ref", &backup, "HEAD"]).await?;
    let applied = if fast_forward {
        repo.git
            .run(&["merge", "--ff-only", "--quiet", &reference])
            .await?
    } else {
        repo.git
            .run(&[
                "merge",
                "--no-edit",
                "--quiet",
                "-m",
                &format!("Merge {} (silo fold)", fork.name),
                &reference,
            ])
            .await?
    };
    if applied.code != 0 {
        if !fast_forward {
            let _ = repo.git.run(&["merge", "--abort"]).await;
        }
        let _ = repo.git.run(&["update-ref", "-d", &backup]).await;
        return Ok(refused(
            &fork.name,
            &target,
            "git",
            format!(
                "git could not apply the commits to {branch}: {}",
                if applied.err.is_empty() {
                    applied.out
                } else {
                    applied.err
                }
            ),
            "Nothing was changed.",
        ));
    }
    let after = repo.git.ok(&["rev-parse", "HEAD"]).await?;
    record.backup = Some(Backup {
        reference: backup.clone(),
        branch: branch.clone(),
        before: repo.head.clone(),
        after,
    });
    save(engine, &record)?;
    Ok(json!({
        "status": "kept",
        "report": {
            "fork": fork.name, "target": target,
            "summary": summary,
            "steps": [
                format!("{} {branch} with {count} commit{}", if fast_forward { "Fast-forwarded" } else { "Merged into" }, if count == "1" { "" } else { "s" }),
                format!("The folder's previous HEAD is {backup}."),
            ],
            "notes": [], "checkpoint": null,
        },
    }))
}

async fn fetch_into(
    engine: &Engine,
    fork: &Computer,
    repo: &Repo,
    remote: &str,
    bundle: &Path,
    label: &str,
    progress: Progress<'_>,
) -> Result<String, CliError> {
    download(engine, fork, remote, bundle, progress).await?;
    let reference = format!("refs/silo/{label}/head");
    let spec = format!("+HEAD:{reference}");
    let bundle_arg = bundle.to_string_lossy().into_owned();
    repo.git.ok(&["fetch", "--quiet", &bundle_arg, &spec]).await.map_err(|error| {
        CliError::new(format!(
            "The fork's commits do not fit the folder's history ({}). Was the folder rewritten after the fork was made?",
            error.message
        ))
    })?;
    Ok(reference)
}

/// The files that conflict in a merge of `reference` into HEAD, or none when it is clean.
async fn conflicts_with(repo: &Repo, reference: &str) -> Result<Option<Vec<String>>, CliError> {
    let tree = repo
        .git
        .run(&[
            "merge-tree",
            "--write-tree",
            "--name-only",
            "HEAD",
            reference,
        ])
        .await?;
    match tree.code {
        0 => Ok(None),
        1 => {
            // After the tree id, a blank line, then the conflicted file names.
            let files: Vec<String> = tree
                .out
                .lines()
                .skip(1)
                .take_while(|line| !line.is_empty())
                .map(str::to_owned)
                .collect();
            Ok(Some(if files.is_empty() {
                vec!["(see git merge-tree)".into()]
            } else {
                files
            }))
        }
        // An older git without `merge-tree --write-tree`: try the merge and take it back.
        _ => {
            let trial = repo
                .git
                .run(&["merge", "--no-commit", "--no-ff", "--quiet", reference])
                .await?;
            let _ = repo.git.run(&["merge", "--abort"]).await;
            if trial.code == 0 {
                Ok(None)
            } else {
                Ok(Some(vec!["(the merge did not apply)".into()]))
            }
        }
    }
}

async fn unique_backup(repo: &Repo) -> Result<String, CliError> {
    let base = format!("refs/silo/backup/{}", utc_stamp());
    let mut candidate = base.clone();
    let mut n = 1;
    while repo
        .git
        .succeeds(&["show-ref", "--verify", "--quiet", &candidate])
        .await?
    {
        n += 1;
        candidate = format!("{base}-{n}");
    }
    Ok(candidate)
}

// MARK: Undo

/// Puts the host folder's branch back to where it was before the latest fold of `record`.
/// Refused when the folder has uncommitted changes to tracked files or has moved on since.
pub async fn undo_folder(engine: &Engine, mut record: Record) -> Result<Value, CliError> {
    let Some(backup) = record.backup.clone() else {
        return Ok(json!({"status": "nothing-to-undo", "computer": record.path}));
    };
    let repo = Repo::open(Path::new(&record.path)).await?;
    let refuse =
        |message: String| json!({"status": "refused", "computer": record.path, "message": message});
    if repo.branch.as_deref() != Some(backup.branch.as_str()) {
        return Ok(refuse(format!(
            "{} is not on {} any more.",
            record.path, backup.branch
        )));
    }
    if repo.dirty().await? {
        return Ok(refuse(format!(
            "{} has uncommitted changes to tracked files.",
            record.path
        )));
    }
    if repo.head != backup.after {
        return Ok(refuse(format!(
            "{} has new commits since the fold; undoing would drop them. To go back anyway: git -C {} reset --hard {}",
            backup.branch, record.path, backup.reference
        )));
    }
    repo.git
        .ok(&["reset", "--hard", "--quiet", &backup.reference])
        .await?;
    record.backup = None;
    save(engine, &record)?;
    Ok(json!({
        "status": "undone",
        "computer": record.path,
        "branch": backup.branch,
        "head": backup.before,
        "fork": record.fork,
    }))
}

/// The record `silo undo <target>` acts on when `target` is a fork or a folder.
pub async fn undo_target(engine: &Engine, target: &str, base: &Path) -> Option<Record> {
    if is_folder(target) {
        let path = std::fs::canonicalize(base.join(target)).ok()?;
        return record_for_path(engine, &path);
    }
    let computer = engine.computer(target).await.ok()?;
    record_for(engine, &computer.id)
}

// MARK: Review

/// Names the fork's repository state: its HEAD and a short hash of the uncommitted changes.
pub async fn review_digest(
    engine: &Engine,
    fork: &Computer,
    record: &Record,
) -> Result<String, CliError> {
    let script = "cd \"$1\" && h=$(git rev-parse HEAD) && \
                  d=$( (git diff HEAD; git ls-files -o --exclude-standard) | sha256sum | cut -c1-16) && \
                  printf '%s-%s' \"$h\" \"$d\"";
    guest(
        engine,
        &fork.name,
        script,
        &[&record.guest_path],
        "Reading the repository",
    )
    .await
}

/// The fork's commits since the folder's and its uncommitted changes, as short text.
pub async fn review_summary(
    engine: &Engine,
    fork: &Computer,
    record: &Record,
) -> Result<String, CliError> {
    let script =
        "cd \"$1\" && echo \"Commits since $2:\" && (git log --oneline \"$2..HEAD\" | head -20) \
                  && echo \"Uncommitted:\" && (git status --short | head -20)";
    guest(
        engine,
        &fork.name,
        script,
        &[&record.guest_path, &record.base],
        "Reading the repository",
    )
    .await
}
