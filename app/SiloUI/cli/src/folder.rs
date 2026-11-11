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
    engine::{Computer, Engine, Progress},
    error::CliError,
    ops::{self, ForkArgs},
};

/// The CLI memory file that records which forks hold a host folder.
const MEMORY_FILE: &str = "cli-folders.json";
/// The guest folder the transfer API allows besides Downloads; bundles pass through a
/// folder of their own below it, which is removed afterwards.
pub(crate) const TRANSFER_ROOT: &str = "/workspace";
/// A folder of the repository's git directory that holds bundles in transit.
const TRANSIT: &str = "silo";
/// Said when the fork moved after it was reviewed.
pub const CHANGED: &str = "The fork changed since it was reviewed; review it again.";

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
    )?;
    // The engine reads the record from its channel's state directory, whichever socket this
    // command was given, to know which forks hold a folder.
    if let Ok(home) = crate::access::home_dir() {
        let _ = mirror_record(&crate::engine::build_channel().state_dir(&home), record);
    }
    Ok(())
}

/// Writes the fork's folder record into `state_dir`'s copy of the memory file, when that
/// directory exists (it is the engine's own; a missing one is not made).
pub fn mirror_record(state_dir: &Path, record: &Record) -> Result<(), CliError> {
    if !state_dir.is_dir() {
        return Ok(());
    }
    let file = state_dir.join(MEMORY_FILE);
    let mut all: serde_json::Map<String, Value> = std::fs::read(&file)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    all.insert(
        record.fork_id.clone(),
        serde_json::to_value(record).unwrap_or_default(),
    );
    let bytes = serde_json::to_vec_pretty(&all)
        .map_err(|error| CliError::new(format!("Cannot save {MEMORY_FILE}: {error}")))?;
    std::fs::write(&file, bytes)
        .map_err(|error| CliError::new(format!("Cannot save {}: {error}", file.display())))
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

/// Settings every host git command runs with. Git's own hooks, the file system monitor
/// hook and external transports are programs the folder's configuration can name; a fork's
/// commits are not trusted to cause any of them to run, so they are switched off for the
/// commands `merge` and `undo` run. The folder's identity and other settings still apply, so
/// the merge commit is made as its owner. A merge commit made here does not run the
/// folder's own commit hooks either.
const HARDENING: [&str; 6] = [
    "-c",
    "core.hooksPath=/dev/null",
    "-c",
    "core.fsmonitor=false",
    "-c",
    "protocol.ext.allow=never",
];

pub(crate) struct Git {
    pub(crate) dir: PathBuf,
}

pub(crate) struct GitOutput {
    pub(crate) code: i32,
    pub(crate) out: String,
    pub(crate) err: String,
}

impl Git {
    pub(crate) async fn run(&self, args: &[&str]) -> Result<GitOutput, CliError> {
        let mut output = self.run_untrimmed(args).await?;
        output.out.truncate(output.out.trim_end().len());
        output.err.truncate(output.err.trim_end().len());
        Ok(output)
    }

    async fn run_untrimmed(&self, args: &[&str]) -> Result<GitOutput, CliError> {
        self.run_env(args, &[]).await
    }

    /// Runs the command with extra environment variables (the push credential header).
    pub(crate) async fn run_env(
        &self,
        args: &[&str],
        env: &[(String, String)],
    ) -> Result<GitOutput, CliError> {
        let dir = self.dir.clone();
        let env = env.to_vec();
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        tokio::task::spawn_blocking(move || {
            let mut command = std::process::Command::new("git");
            command
                .arg("-C")
                .arg(&dir)
                .args(HARDENING)
                .args(&args)
                .env_remove("GIT_DIR")
                .env_remove("GIT_EXTERNAL_DIFF")
                .env_remove("GIT_CONFIG_PARAMETERS")
                .env_remove("GIT_ASKPASS")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_INDEX_FILE")
                .env("GIT_TERMINAL_PROMPT", "0")
                .envs(env);
            command.output().map(|output| GitOutput {
                code: output.status.code().unwrap_or(-1),
                out: String::from_utf8_lossy(&output.stdout).into_owned(),
                err: String::from_utf8_lossy(&output.stderr).into_owned(),
            })
        })
        .await
        .map_err(|_| CliError::new("git stopped unexpectedly."))?
        .map_err(|error| CliError::new(format!("Cannot run git: {error}")))
    }

    /// The output of a git command that must succeed.
    pub(crate) async fn ok(&self, args: &[&str]) -> Result<String, CliError> {
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

    /// The raw output of a git command that must succeed, not trimmed (for `-z` output).
    async fn ok_raw(&self, args: &[&str]) -> Result<String, CliError> {
        let output = self.run_untrimmed(args).await?;
        if output.code == 0 {
            Ok(output.out)
        } else {
            Err(CliError::new(format!(
                "git {} failed: {}",
                args.first().copied().unwrap_or_default(),
                output.err.trim_end()
            )))
        }
    }

    pub(crate) async fn succeeds(&self, args: &[&str]) -> Result<bool, CliError> {
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

pub(crate) fn clean_up(path: &Path) {
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

pub(crate) async fn guest(
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

pub(crate) fn transfer_id() -> String {
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

pub(crate) async fn download(
    engine: &Engine,
    computer: &Computer,
    path: &str,
    destination: &Path,
    progress: Progress<'_>,
) -> Result<(), CliError> {
    // A fork that was just started or used can refuse the first connection of the transfer
    // while its SSH service settles; the second try comes a few seconds later.
    let mut attempt = 0;
    let outcome = loop {
        attempt += 1;
        let tried = engine
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
            .await;
        match tried {
            Err(error) if attempt < 3 && error.message.starts_with("Could not connect") => {
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
            other => break other?,
        }
    };
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
    let result = send(engine, &bundle, &name, &label, secrets, progress).await;
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
            recipe_name: Some(crate::recipe_file::DEFAULT_RECIPE.to_owned()),
            no_start: false,
        },
        progress,
    )
    .await?;
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
    let inbox = format!("{TRANSFER_ROOT}/silo-in-{}", uuid::Uuid::new_v4().simple());
    let guest_path = format!("{home}/{label}");
    guest(
        engine,
        name,
        "mkdir -p \"$1\"",
        &[&inbox],
        "Preparing the transfer",
    )
    .await?;
    let uploaded = upload(engine, &fork, &inbox, bundle, progress).await;
    if let Err(error) = uploaded {
        let _ = guest(engine, name, "rm -rf \"$1\"", &[&inbox], "Cleaning up").await;
        return Err(error);
    }
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
    let cloned = guest(
        engine,
        name,
        setup,
        &[&remote, &guest_path, &inbox],
        "Cloning the repository",
    )
    .await;
    if let Err(error) = cloned {
        let _ = guest(engine, name, "rm -rf \"$1\"", &[&inbox], "Cleaning up").await;
        return Err(error);
    }
    Ok((fork, guest_path))
}

// MARK: Fold

/// Shell that names the fork's working state without touching its index: the tree git would
/// commit after `add -A`, built in a copy of the index. It sets `$h` (HEAD) and `$tree`.
/// Hooks are off, and file contents, binary or not, decide the tree.
const STATE_TREE: &str = "idx=$(git rev-parse --git-path index); \
    t=$(mktemp \"${TMPDIR:-/tmp}/silo-index.XXXXXX\"); trap 'rm -f \"$t\"' EXIT; \
    if [ -f \"$idx\" ]; then cp \"$idx\" \"$t\"; else rm -f \"$t\"; fi; \
    GIT_INDEX_FILE=$t git -c core.hooksPath=/dev/null add -A >/dev/null; \
    tree=$(GIT_INDEX_FILE=$t git write-tree); h=$(git rev-parse HEAD); ";

fn refused(fork: &str, target: &str, reason: &str, message: String, hint: &str) -> Value {
    json!({
        "status": "refused",
        "report": {"fork": fork, "target": target, "summary": [], "steps": [], "notes": [], "checkpoint": null},
        "refusal": {"reason": reason, "message": message, "conflicts": [], "hint": hint},
    })
}

pub(crate) fn ref_label(fork: &str) -> String {
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
    record: Record,
    dry_run: bool,
    reviewed: Option<&str>,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    // The commits are read from the running fork: a stopped one is started for this and put
    // back to stopped afterwards.
    if fork.state == "running" {
        return fold_running_folder(engine, fork, record, dry_run, reviewed, progress).await;
    }
    if fork.state != "stopped" {
        return Err(CliError::new(format!(
            "{} is {}; wait until it is running or stopped, then merge again.",
            fork.name, fork.state
        )));
    }
    ops::start(engine, &fork.name, progress).await?;
    let started = engine.computer(&fork.name).await?;
    let result = fold_running_folder(engine, &started, record, dry_run, reviewed, progress).await;
    let stopped = ops::stop(engine, &fork.name, progress).await;
    let value = result?;
    stopped?;
    Ok(value)
}

/// Commits the fork's uncommitted work, bundles the commits since the folder's, and fetches
/// them into `refs/silo/<fork>/head` of the folder; that reference, or `None` when the fork
/// has nothing beyond the commit it started from.
async fn gather_commits(
    engine: &Engine,
    fork: &Computer,
    record: &Record,
    repo: &Repo,
    expect: Option<&str>,
    progress: Progress<'_>,
) -> Result<Option<String>, CliError> {
    // Uncommitted work in the fork is committed first, so the fork's files are not left out.
    // With `expect`, the state is compared with the reviewed digest in the same script that
    // commits it, so nothing that changed in between is brought along.
    let outbox = format!("{TRANSFER_ROOT}/silo-out-{}", uuid::Uuid::new_v4().simple());
    let remote_bundle = format!("{outbox}/out.bundle");
    let gather = format!(
        "set -e; cd \"$1\"; {STATE_TREE} \
         if [ -n \"$5\" ]; then [ \"$h-$tree\" = \"$5\" ] || {{ echo changed; exit 0; }}; fi; \
         if [ \"$tree\" != \"$(git rev-parse 'HEAD^{{tree}}')\" ]; then \
         c=$(git -c user.name=\"$(git config user.name || echo Silo)\" \
         -c user.email=\"$(git config user.email || echo silo@localhost)\" \
         commit-tree \"$tree\" -p HEAD -m \"silo: uncommitted changes in $3\"); \
         git update-ref HEAD \"$c\"; git reset -q; fi; \
         head=$(git rev-parse HEAD); \
         if [ \"$head\" = \"$2\" ]; then echo unchanged; else \
         mkdir -p \"$4\"; git bundle create --quiet \"$4/out.bundle\" \"$2..HEAD\"; echo \"$head\"; fi"
    );
    let produced = guest(
        engine,
        &fork.name,
        &gather,
        &[
            &record.guest_path,
            &record.base,
            &fork.name,
            &outbox,
            expect.unwrap_or(""),
        ],
        "Collecting the commits",
    )
    .await?;
    if produced == "changed" {
        let _ = guest(
            engine,
            &fork.name,
            "rm -rf \"$1\"",
            &[&outbox],
            "Cleaning up",
        )
        .await;
        return Err(CliError::new(CHANGED));
    }
    if produced == "unchanged" {
        return Ok(None);
    }
    let bundle = repo.transit(&format!("{}.bundle", transfer_id()))?;
    let fetched = fetch_into(
        engine,
        fork,
        repo,
        &remote_bundle,
        &bundle,
        &ref_label(&fork.name),
        progress,
    )
    .await;
    clean_up(&bundle);
    let _ = guest(
        engine,
        &fork.name,
        "rm -rf \"$1\"",
        &[&outbox],
        "Cleaning up",
    )
    .await;
    Ok(Some(fetched?))
}

async fn fold_running_folder(
    engine: &Engine,
    fork: &Computer,
    mut record: Record,
    dry_run: bool,
    reviewed: Option<&str>,
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
            "Check out a branch there, then merge again.",
        ));
    };
    if repo.dirty().await? {
        return Ok(refused(
            &fork.name,
            &target,
            "dirty",
            format!("{target} has uncommitted changes to tracked files."),
            "Commit or stash them, then merge again.",
        ));
    }
    let gathered = gather_commits(engine, fork, &record, &repo, reviewed, progress).await;
    if matches!(&gathered, Err(error) if error.message == CHANGED) {
        return Ok(refused(
            &fork.name,
            &target,
            "changed",
            CHANGED.into(),
            "Run `silo diff` again, then merge.",
        ));
    }
    let Some(reference) = gathered? else {
        return Ok(json!({
            "status": "unchanged",
            "report": {"fork": fork.name, "target": target, "summary": [],
                       "steps": ["The fork has no commits beyond the folder's; nothing to bring back."],
                       "notes": [], "checkpoint": null},
        }));
    };

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
    let range = format!("HEAD...{reference}");
    let coming = incoming(&repo, &range).await?;
    if !coming.attributes.is_empty() {
        return Ok(refused(
            &fork.name,
            &target,
            "attributes",
            format!(
                "The fork changes git attributes ({}), which can select filter or merge programs configured in {target}; nothing was changed.",
                list(&coming.attributes)
            ),
            &format!("Review the change with `git -C {target} diff {range}` and merge {reference} yourself."),
        ));
    }
    if !coming.in_the_way.is_empty() {
        return Ok(refused(
            &fork.name,
            &target,
            "untracked",
            format!(
                "{target} has files git does not track (untracked or ignored) where the fork's commits add files: {}. Keeping would overwrite them; nothing was changed.",
                list(&coming.in_the_way)
            ),
            "Move or remove those files, then merge again.",
        ));
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
            .run(&["merge", "--ff-only", "--quiet", "--no-verify", &reference])
            .await?
    } else {
        repo.git
            .run(&[
                "merge",
                "--no-edit",
                "--quiet",
                "--no-verify",
                "-m",
                &format!("Merge {} (silo merge)", fork.name),
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
                .run(&[
                    "merge",
                    "--no-commit",
                    "--no-ff",
                    "--quiet",
                    "--no-verify",
                    reference,
                ])
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

/// Paths the commits in `range` (a `git diff` range) leave in the tree, split in the ones
/// that are `.gitattributes` files (they can name filter or merge drivers the folder's
/// configuration defines as commands) and the ones where the folder already has a file
/// that git does not track (untracked or ignored), which checking the commits out would
/// overwrite without a trace.
struct Incoming {
    attributes: Vec<String>,
    in_the_way: Vec<String>,
}

async fn incoming(repo: &Repo, range: &str) -> Result<Incoming, CliError> {
    let changed = repo
        .git
        .ok_raw(&[
            "diff",
            "--name-only",
            "-z",
            "--no-renames",
            "--diff-filter=d",
            range,
        ])
        .await?;
    let paths: Vec<&str> = changed.split('\0').filter(|p| !p.is_empty()).collect();
    let attributes = paths
        .iter()
        .filter(|path| path.rsplit('/').next() == Some(".gitattributes"))
        .map(|path| (*path).to_owned())
        .collect();
    let present: Vec<&str> = paths
        .iter()
        .copied()
        .filter(|path| repo.git.dir.join(path).symlink_metadata().is_ok())
        .collect();
    let mut tracked = std::collections::HashSet::new();
    for chunk in present.chunks(200) {
        let mut args = vec!["ls-files", "-z", "--cached", "--"];
        let specs: Vec<String> = chunk
            .iter()
            .map(|path| format!(":(literal){path}"))
            .collect();
        args.extend(specs.iter().map(String::as_str));
        let listed = repo.git.ok_raw(&args).await?;
        tracked.extend(
            listed
                .split('\0')
                .filter(|p| !p.is_empty())
                .map(str::to_owned),
        );
    }
    Ok(Incoming {
        attributes,
        in_the_way: present
            .into_iter()
            .filter(|path| !tracked.contains(*path))
            .map(str::to_owned)
            .collect(),
    })
}

fn list(paths: &[String]) -> String {
    let mut text = paths
        .iter()
        .take(10)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if paths.len() > 10 {
        text.push_str(&format!(", and {} more", paths.len() - 10));
    }
    text
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
            "{} has new commits since the merge; undoing would drop them. To go back anyway: git -C {} reset --hard {}",
            backup.branch, record.path, backup.reference
        )));
    }
    let coming = incoming(&repo, &format!("HEAD..{}", backup.reference)).await?;
    if !coming.in_the_way.is_empty() {
        return Ok(refuse(format!(
            "{} has files git does not track (untracked or ignored) where undoing restores files: {}. Nothing was changed; move them first.",
            record.path,
            list(&coming.in_the_way)
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
    let script = format!("set -e; cd \"$1\"; {STATE_TREE} printf '%s-%s' \"$h\" \"$tree\"");
    guest(
        engine,
        &fork.name,
        &script,
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

/// Whether the fork has commits beyond the ones it started from or uncommitted changes.
pub async fn has_changes(
    engine: &Engine,
    fork: &Computer,
    record: &Record,
) -> Result<bool, CliError> {
    let script = "cd \"$1\" && if [ \"$(git rev-list --count \"$2..HEAD\")\" != 0 ] \
                  || [ -n \"$(git status --porcelain)\" ]; then echo yes; else echo no; fi";
    let answer = guest(
        engine,
        &fork.name,
        script,
        &[&record.guest_path, &record.base],
        "Reading the repository",
    )
    .await?;
    Ok(answer == "yes")
}

/// The host folder with the fork's work on a branch, ready to push.
pub struct PrSource {
    pub directory: PathBuf,
    pub branch: String,
    /// The branch the folder is on: where a pull request would merge.
    pub base: Option<String>,
    /// The folder's `origin` URL.
    pub remote: String,
}

/// Puts the fork's commits (its uncommitted work committed first, as a fold does) on a new
/// branch `branch` of the host folder. Nothing else in the folder changes: the checked-out
/// branch and the working tree stay as they are. A stopped fork is started for this and put
/// back to stopped.
pub async fn branch_for_pr(
    engine: &Engine,
    fork: &Computer,
    record: &Record,
    branch: &str,
    base: Option<&str>,
    progress: Progress<'_>,
) -> Result<PrSource, CliError> {
    let started_here = match fork.state.as_str() {
        "running" => false,
        "stopped" => {
            ops::start(engine, &fork.name, progress).await?;
            true
        }
        other => {
            return Err(CliError::new(format!(
                "{} is {other}; wait until it is running or stopped, then try again.",
                fork.name
            )))
        }
    };
    let current = engine.computer(&fork.name).await?;
    let made = make_branch(engine, &current, record, branch, base, progress).await;
    if started_here {
        let stopped = ops::stop(engine, &fork.name, progress).await;
        let source = made?;
        stopped?;
        return Ok(source);
    }
    made
}

async fn make_branch(
    engine: &Engine,
    fork: &Computer,
    record: &Record,
    branch: &str,
    base: Option<&str>,
    progress: Progress<'_>,
) -> Result<PrSource, CliError> {
    let repo = Repo::open(Path::new(&record.path)).await.map_err(|error| {
        CliError::new(format!(
            "The folder {} that {} was made from cannot be used: {}",
            record.path, fork.name, error.message
        ))
    })?;
    let remote = repo.git.run(&["remote", "get-url", "origin"]).await?;
    if remote.code != 0 || remote.out.is_empty() {
        return Err(CliError::new(format!(
            "{} has no origin remote to push the branch to.",
            record.path
        )));
    }
    let base = base
        .map(str::to_owned)
        .or_else(|| repo.branch.clone())
        .ok_or_else(|| {
            CliError::new(format!(
                "{} has no branch checked out; pass --base.",
                record.path
            ))
        })?;
    // The pull request is the fork's starting commit plus the fork's commits: commits up to
    // that start that the host has not pushed would be published with it.
    let upstream = format!("origin/{base}");
    if !repo
        .git
        .succeeds(&["rev-parse", "--verify", "-q", &upstream])
        .await?
    {
        return Err(CliError::new(format!(
            "{} has no {upstream} to compare with; fetch it first, or pass --base.",
            record.path
        )));
    }
    let unpushed = repo
        .git
        .ok(&[
            "log",
            "--oneline",
            "--max-count=20",
            &format!("{upstream}..{}", record.base),
        ])
        .await?;
    if !unpushed.is_empty() {
        return Err(CliError::new(format!(
            "{} has commits that {upstream} does not, and the pull request would carry them:\n{unpushed}\nPush or drop them first.",
            record.path
        )));
    }
    let Some(reference) = gather_commits(engine, fork, record, &repo, None, progress).await? else {
        return Err(CliError::new(format!(
            "{} has no commits beyond the folder's; there is nothing to open a pull request for.",
            fork.name
        )));
    };
    repo.git.ok(&["branch", branch, &reference]).await?;
    Ok(PrSource {
        directory: repo.git.dir.clone(),
        branch: branch.to_owned(),
        base: Some(base),
        remote: remote.out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn the_folder_record_is_mirrored_into_an_existing_channel_state_directory_only() {
        let record: Record = serde_json::from_value(json!({
            "forkId": "f1", "fork": "try", "path": "/work/app", "guestPath": "/root/app",
            "base": "abc", "branch": "main", "backup": null
        }))
        .unwrap();
        let state = tempfile::tempdir().unwrap();
        std::fs::write(
            state.path().join(MEMORY_FILE),
            r#"{"other": {"path": "/x"}}"#,
        )
        .unwrap();
        mirror_record(state.path(), &record).unwrap();
        let written: Value =
            serde_json::from_slice(&std::fs::read(state.path().join(MEMORY_FILE)).unwrap())
                .unwrap();
        assert_eq!(written["f1"]["path"], "/work/app");
        assert_eq!(written["other"]["path"], "/x");
        // A directory that does not exist is not made.
        let missing = state.path().join("nowhere");
        mirror_record(&missing, &record).unwrap();
        assert!(!missing.exists());
    }

    fn run_git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.com",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    fn repo_with(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        run_git(dir.path(), &["init", "-q", "-b", "main"]);
        std::fs::write(dir.path().join(".gitignore"), "ignored.txt\n").unwrap();
        for (name, text) in files {
            std::fs::write(dir.path().join(name), text).unwrap();
        }
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-q", "-m", "base"]);
        dir
    }

    #[tokio::test]
    async fn incoming_names_untracked_and_ignored_files_the_commits_would_overwrite() {
        let dir = repo_with(&[("a.txt", "a")]);
        run_git(dir.path(), &["checkout", "-q", "-b", "fork"]);
        for name in ["ignored.txt", "loose.txt", "fresh.txt"] {
            std::fs::write(dir.path().join(name), "fork").unwrap();
        }
        run_git(
            dir.path(),
            &["add", "-f", "ignored.txt", "loose.txt", "fresh.txt"],
        );
        run_git(dir.path(), &["commit", "-q", "-m", "fork"]);
        run_git(dir.path(), &["checkout", "-q", "main"]);
        std::fs::write(dir.path().join("ignored.txt"), "mine").unwrap();
        std::fs::write(dir.path().join("loose.txt"), "mine").unwrap();
        let repo = Repo::open(dir.path()).await.unwrap();
        let coming = incoming(&repo, "HEAD...fork").await.unwrap();
        assert_eq!(coming.in_the_way, ["ignored.txt", "loose.txt"]);
        assert!(coming.attributes.is_empty());
    }

    #[tokio::test]
    async fn incoming_flags_changed_gitattributes_files() {
        let dir = repo_with(&[("a.txt", "a")]);
        run_git(dir.path(), &["checkout", "-q", "-b", "fork"]);
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/.gitattributes"), "* filter=x\n").unwrap();
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-q", "-m", "attrs"]);
        run_git(dir.path(), &["checkout", "-q", "main"]);
        let repo = Repo::open(dir.path()).await.unwrap();
        let coming = incoming(&repo, "HEAD...fork").await.unwrap();
        assert_eq!(coming.attributes, ["sub/.gitattributes"]);
    }

    #[tokio::test]
    async fn undo_would_overwrite_files_the_fold_had_deleted() {
        let dir = repo_with(&[("a.txt", "a"), ("b.txt", "b")]);
        run_git(dir.path(), &["branch", "backup"]);
        run_git(dir.path(), &["rm", "-q", "b.txt"]);
        run_git(dir.path(), &["commit", "-q", "-m", "delete"]);
        std::fs::write(dir.path().join("b.txt"), "my new work").unwrap();
        let repo = Repo::open(dir.path()).await.unwrap();
        let coming = incoming(&repo, "HEAD..backup").await.unwrap();
        assert_eq!(coming.in_the_way, ["b.txt"]);
        std::fs::remove_file(dir.path().join("b.txt")).unwrap();
        assert!(incoming(&repo, "HEAD..backup")
            .await
            .unwrap()
            .in_the_way
            .is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn host_git_runs_no_hooks_even_when_the_folder_configures_them() {
        use std::os::unix::fs::PermissionsExt;
        let dir = repo_with(&[("a.txt", "a")]);
        let outside = tempfile::tempdir().unwrap();
        let marker = outside.path().join("hook-ran");
        let hooks = outside.path().join("my-hooks");
        std::fs::create_dir(&hooks).unwrap();
        for name in [
            "post-merge",
            "pre-merge-commit",
            "post-checkout",
            "reference-transaction",
        ] {
            let hook = hooks.join(name);
            std::fs::write(
                &hook,
                format!("#!/bin/sh\necho \"$0 $@\" >> '{}'\n", marker.display()),
            )
            .unwrap();
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        run_git(
            dir.path(),
            &["config", "core.hooksPath", hooks.to_str().unwrap()],
        );
        run_git(dir.path(), &["checkout", "-q", "-b", "fork"]);
        std::fs::write(dir.path().join("b.txt"), "b").unwrap();
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-q", "-m", "fork"]);
        run_git(dir.path(), &["checkout", "-q", "main"]);
        std::fs::remove_file(&marker).ok();
        let repo = Repo::open(dir.path()).await.unwrap();
        let merged = repo
            .git
            .run(&["merge", "--ff-only", "--quiet", "--no-verify", "fork"])
            .await
            .unwrap();
        assert_eq!(merged.code, 0, "{}", merged.err);
        assert!(dir.path().join("b.txt").exists());
        assert!(
            !marker.exists(),
            "a hook ran: {:?}",
            std::fs::read_to_string(&marker)
        );
    }

    #[test]
    fn transfer_folders_are_in_the_roots_the_transfer_api_allows() {
        assert_eq!(TRANSFER_ROOT, "/workspace");
    }
}
