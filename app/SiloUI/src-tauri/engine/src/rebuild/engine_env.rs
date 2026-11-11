//! The engine's [`RebuildEnv`]: computers are created and removed through the saved computer
//! configuration (so the build sandbox is a computer like any other while it exists), the
//! checkpoint is captured directly, start and stop run the runtime's lifecycle, and all of it
//! happens under the operation gate the rebuild holds for the old computer from first step to
//! last (the gate does not nest, so none of these takes it again). The workspace disk and the
//! SSH folder move on the host, and the home folder travels as a tar file through `msb exec`
//! and `msb cp`.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};

use silo_api::methods::checkpoints::CheckpointKind;

use serde_json::Value;
use silo_api::methods::recipes::RecipeSpec;

use super::{journal::Plan, swap, Info, RebuildEnv};
use crate::{
    changes::{
        keep::{
            guest_env::GuestEnv, journal::Journal as KeepJournal, CheckpointRef, KeepEnv, Os,
        },
        promote::engine_env::stored_under_its_name,
    },
    checkpoints::Service,
    guest::{self, ExecRequest, GuestUser},
    macos_computers,
    recipes::{self, BuildStep, ExecOut, RunAs},
    runtime::{
        self, ComputerConfiguration, ComputerConfigurationChange, ProcessRunner, RuntimeHost,
        RuntimePaths, RuntimeRunner,
    },
};

/// The longest one recipe step may run.
const STEP_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// The longest packing or unpacking of the home folder may take.
const TAR_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const COPY_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Packs the home folder into the tar file named by the first argument, inside the old
/// computer, and reports `<uid> <gid> <sha256>` of the working account and the file. Exit 1
/// from `tar` means a file changed while it was read; the archive is still usable. Sparse
/// files stay sparse, and owners, modes, links and extended attributes are kept. The file is
/// created exclusively and readable by root only.
pub(super) const PACK_HOME: &str = r#"set -eu
umask 077
file=$1
rc=0
set -C
tar -C /home -cp --sparse --xattrs --acls --numeric-owner -f - silo > "$file" || rc=$?
set +C
[ "$rc" -le 1 ] || exit "$rc"
[ -s "$file" ]
tar -tf "$file" > /dev/null
echo "$(id -u silo) $(id -g silo) $(sha256sum "$file" | cut -d' ' -f1)"
"#;

/// Unpacks the tar file named by the first argument over the new computer's home folder, which
/// the working account's setup already made. The other arguments are what [`PACK_HOME`]
/// reported: the working account must have the same numeric owner here, and the file must be
/// the one that was packed.
pub(super) const UNPACK_HOME: &str = r#"set -eu
file=$1
uid=$2
gid=$3
sum=$4
if [ "$(id -u silo)" != "$uid" ] || [ "$(id -g silo)" != "$gid" ]; then
  echo "the working account here has another user or group id than in the old computer" >&2
  exit 3
fi
if [ "$(sha256sum "$file" | cut -d' ' -f1)" != "$sum" ]; then
  echo "the home folder arrived damaged" >&2
  exit 4
fi
tar -tf "$file" > /dev/null
tar -C /home -xpf "$file" --xattrs --acls --numeric-owner --same-owner
rm -f -- "$file"
sync
"#;

const REMOVE_TAR: &str = r#"rm -f -- "$1""#;

/// What [`PACK_HOME`] reported: the working account's user and group id, and the file's
/// SHA-256.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct PackReport {
    pub uid: String,
    pub gid: String,
    pub sum: String,
}

pub(super) fn parse_pack_report(stdout: &[u8]) -> Result<PackReport, String> {
    let text = String::from_utf8_lossy(stdout);
    let line = text.lines().rev().find(|line| !line.trim().is_empty());
    let fields: Vec<&str> = line.map(|l| l.split_whitespace().collect()).unwrap_or_default();
    match fields.as_slice() {
        [uid, gid, sum]
            if uid.chars().all(|c| c.is_ascii_digit())
                && gid.chars().all(|c| c.is_ascii_digit())
                && sum.len() == 64
                && sum.chars().all(|c| c.is_ascii_hexdigit()) =>
        {
            Ok(PackReport {
                uid: (*uid).to_owned(),
                gid: (*gid).to_owned(),
                sum: (*sum).to_owned(),
            })
        }
        _ => Err("The home folder could not be packed completely.".into()),
    }
}

/// Where the home folder's tar file waits inside the computers: a name nobody can guess.
fn guest_tar(plan: &Plan) -> String {
    format!("/tmp/silo-home-{}.tar", plan.undo_id)
}

/// The staging file on the host, removed when it goes out of scope.
struct Staged(PathBuf);

impl Drop for Staged {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub struct EngineRebuildEnv {
    host: Arc<dyn RuntimeHost>,
    guest: GuestEnv,
    /// Where the home folder's tar file waits on the host between the two computers.
    stage_dir: PathBuf,
}

impl EngineRebuildEnv {
    pub fn new(
        host: Arc<dyn RuntimeHost>,
        service: Service,
        keep_journal: KeepJournal,
        stage_dir: PathBuf,
    ) -> Self {
        let guest = GuestEnv::new(host.clone(), service, keep_journal, Box::new(|_| {}));
        Self {
            host,
            guest,
            stage_dir,
        }
    }

    fn paths(&self) -> Result<RuntimePaths, String> {
        runtime::runtime_paths(self.host.as_ref())
    }

    fn inventory(&self) -> Result<Vec<ComputerConfiguration>, String> {
        let paths = self.paths()?;
        Ok(runtime::read_metadata(&paths.metadata)
            .map_err(|error| error.to_string())?
            .computers)
    }

    fn configuration(&self, id: &str) -> Result<ComputerConfiguration, String> {
        self.inventory()?
            .into_iter()
            .find(|computer| computer.id() == id)
            .ok_or_else(|| "This computer no longer exists on this device.".to_owned())
    }

    fn name_of(&self, id: &str) -> Result<String, String> {
        Ok(self.configuration(id)?.name().to_owned())
    }

    fn target(&self, id: &str) -> Result<guest::Target, String> {
        let context = self.host.engine_context();
        guest::resolve(self.paths(), &context.paths().app_data, id).map_err(|error| error.message)
    }

    fn stage_file(&self, plan: &Plan) -> PathBuf {
        self.stage_dir.join(super::stage_file_name(&plan.stage))
    }

    /// Runs a script as root in the running computer and returns its output.
    fn root_script(
        &self,
        id: &str,
        script: &str,
        args: &[&str],
        timeout: Duration,
    ) -> Result<guest::ExecOutput, String> {
        run_root(&self.target(id)?, script, args, timeout)
    }
}

fn run_root(
    target: &guest::Target,
    script: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<guest::ExecOutput, String> {
    let mut argv = vec!["sh".to_owned(), "-c".to_owned(), script.to_owned(), "silo-rebuild".to_owned()];
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    guest::run_bounded(
        target,
        &ExecRequest {
            argv,
            cwd: None,
            env: Vec::new(),
            user: GuestUser::Root,
            stdin: None,
            timeout,
            max_stdout: 1024 * 1024,
        },
    )
    .map_err(|error| error.message)
}

/// The last 400 characters of what a command said, standard error first.
fn tail(stdout: &[u8], stderr: &[u8]) -> String {
    let text = if stderr.is_empty() { stdout } else { stderr };
    let text = String::from_utf8_lossy(text);
    let text = text.trim();
    let skip = text.chars().count().saturating_sub(400);
    text.chars().skip(skip).collect()
}

fn checked(
    what: &str,
    output: Result<guest::ExecOutput, String>,
) -> Result<guest::ExecOutput, String> {
    let output = output.map_err(|error| format!("{what} failed: {error}"))?;
    if output.exit_code != Some(0) {
        return Err(format!(
            "{what} failed: {}",
            tail(&output.stdout, &output.stderr)
        ));
    }
    Ok(output)
}

/// The size of `/home/silo` in bytes, from the output of `du -sb`.
pub(super) fn parse_du(output: &[u8]) -> Result<u64, String> {
    String::from_utf8_lossy(output)
        .split_whitespace()
        .next()
        .and_then(|bytes| bytes.parse().ok())
        .ok_or_else(|| "The size of the home folder could not be read.".to_owned())
}

/// Runs the recipe's steps one after the other through `run`; the first that fails ends the
/// replay with its label and what it said.
pub(super) fn replay_steps(
    steps: &[BuildStep],
    note: &dyn Fn(&str),
    run: &mut dyn FnMut(&BuildStep) -> Result<ExecOut, String>,
) -> Result<(), String> {
    for step in steps {
        note(&step.label);
        let out = run(step).map_err(|error| format!("{} failed: {error}", step.label))?;
        if out.code != 0 {
            let said = tail(&out.stdout, &out.stderr);
            return Err(format!(
                "{} failed (exit {}): {said}",
                step.label, out.code
            ));
        }
    }
    Ok(())
}

/// Whether `directory`'s disk copies files by reference (APFS, btrfs, XFS with reflinks).
pub(super) fn can_clone(directory: &Path) -> bool {
    let probe = uuid::Uuid::new_v4().simple().to_string();
    let source = directory.join(format!(".silo-clone-probe-{probe}"));
    let copy = directory.join(format!(".silo-clone-probe-{probe}-copy"));
    let ok = fs::write(&source, b"probe").is_ok() && clone_command(&source, &copy);
    let _ = fs::remove_file(&source);
    let _ = fs::remove_file(&copy);
    ok
}

/// Copies a file or folder as a clone (no data is copied) and keeps its attributes. Fails
/// rather than copying the data when the disk cannot clone.
pub(super) fn clone_command(source: &Path, destination: &Path) -> bool {
    let mut command = Command::new("cp");
    if cfg!(target_os = "macos") {
        command.args(["-c", "-R", "-p"]);
    } else {
        command.args(["-a", "--reflink=always"]);
    }
    command
        .arg(source)
        .arg(destination)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Replaces the workspace disk of the stopped sandbox folder `to` with a clone of the one in
/// `from`: the whole `owned-volumes/<id>` folder, and the layer list of the runtime's disk
/// journal (`runtime/owned-disks/<id>/disk.json`) with `to`'s own volume id kept. The folder
/// that was there waits in `.workspace-before-swap`, outside `owned-volumes`, where the
/// runtime would take it for part of the disk.
pub(super) fn clone_workspace(from: &Path, to: &Path) -> Result<(), String> {
    let volume = workspace_volume(&from.join("owned-volumes"))?;
    let source = from.join("owned-volumes").join(&volume);
    let target = to.join("owned-volumes").join(&volume);
    if !target.is_dir() {
        return Err("The new computer has no workspace disk to replace.".into());
    }
    let source_journal = from
        .join("runtime")
        .join("owned-disks")
        .join(&volume)
        .join("disk.json");
    let target_journal = to
        .join("runtime")
        .join("owned-disks")
        .join(&volume)
        .join("disk.json");
    let mut journal: Option<Value> = match fs::read(&source_journal) {
        Ok(bytes) => Some(
            serde_json::from_slice(&bytes)
                .map_err(|_| "The workspace disk's journal is unreadable.".to_owned())?,
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("Cannot read the workspace disk's journal: {error}")),
    };
    if let Some(journal) = journal.as_mut() {
        // The runtime started the new computer once, so its journal exists with a volume id
        // the replacement keeps.
        let own: Value = serde_json::from_slice(
            &fs::read(&target_journal)
                .map_err(|_| "The new computer's workspace disk has not been started once.")?,
        )
        .map_err(|_| "The new computer's workspace disk journal is unreadable.".to_owned())?;
        journal["volume_id"] = own["volume_id"].clone();
    }
    let staged = to.join(format!(".workspace-swap-{volume}"));
    let aside = to.join(".workspace-before-swap");
    let _ = fs::remove_dir_all(&staged);
    if !clone_command(&source, &staged) {
        let _ = fs::remove_dir_all(&staged);
        return Err("The workspace disk could not be cloned.".into());
    }
    // The marker that says which sandbox owns a disk folder stays the new sandbox's own.
    let owner = target.join(OWNER_MARKER);
    let clone_owner = staged.join(OWNER_MARKER);
    let _ = fs::remove_file(&clone_owner);
    if owner.is_file() {
        if let Err(error) = fs::copy(&owner, &clone_owner) {
            let _ = fs::remove_dir_all(&staged);
            return Err(format!("The workspace disk could not be prepared: {error}"));
        }
    }
    let moved = (|| -> std::io::Result<()> {
        let _ = fs::remove_dir_all(&aside);
        fs::rename(&target, &aside)?;
        fs::rename(&staged, &target)?;
        if let Some(journal) = &journal {
            let temporary = target_journal.with_extension("json.new");
            fs::write(&temporary, serde_json::to_vec(journal).unwrap_or_default())?;
            fs::rename(&temporary, &target_journal)?;
        }
        Ok(())
    })();
    moved.map_err(|error| format!("The workspace disk could not be moved: {error}"))
}

/// The file in a disk folder that names the sandbox that owns it.
const OWNER_MARKER: &str = ".disk-owner";

/// The one `workspace_*` folder in `volumes`.
fn workspace_volume(volumes: &Path) -> Result<String, String> {
    let mut found = fs::read_dir(volumes)
        .map_err(|_| "The computer has no workspace disk.".to_owned())?
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("workspace_"));
    match (found.next(), found.next()) {
        (Some(name), None) => Ok(name),
        _ => Err("The computer does not have exactly one workspace disk.".into()),
    }
}

/// The capacity of the computer's `/workspace` disk in MiB, as the runtime holds it.
fn workspace_capacity_mib(paths: &RuntimePaths, name: &str) -> Option<u64> {
    let inspected = runtime::inspect_computer(&ProcessRunner, paths, name).ok()?;
    inspected
        .config
        .pointer("/mounts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|mount| mount["guest"] == "/workspace")
        .and_then(|mount| mount.pointer("/storage/capacity_mib"))
        .and_then(Value::as_u64)
}

/// Copies a folder of files (the SSH host key and its companions) with their modes. A missing
/// source has nothing to copy.
pub(super) fn copy_folder(from: &Path, to: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    if !from.exists() {
        return Ok(());
    }
    let fail = |error: std::io::Error| format!("The SSH folder could not be copied: {error}");
    fs::create_dir_all(to).map_err(fail)?;
    fs::set_permissions(to, fs::Permissions::from_mode(0o700)).map_err(fail)?;
    for entry in fs::read_dir(from).map_err(fail)?.flatten() {
        let kind = entry.file_type().map_err(fail)?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_folder(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &target).map_err(fail)?;
        } else {
            return Err("The SSH folder holds something other than files.".into());
        }
    }
    Ok(())
}

struct RuntimeSandboxes<'a> {
    paths: &'a RuntimePaths,
}

impl swap::Sandboxes for RuntimeSandboxes<'_> {
    fn owner(&self, name: &str) -> Result<Option<String>, String> {
        runtime::sandbox_machine_id(&ProcessRunner, self.paths, name)
            .map_err(|error| error.to_string())
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        runtime::rename_sandbox(&ProcessRunner, self.paths, from, to)
            .map_err(|error| error.to_string())
    }

    fn relabel(&self, name: &str, id: &str) -> Result<(), String> {
        ProcessRunner
            .run(
                self.paths,
                &[
                    "modify".into(),
                    name.into(),
                    "--label".into(),
                    format!("silo.machine-id={id}"),
                ],
                Duration::from_secs(60),
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

impl EngineRebuildEnv {
    /// Starts or stops the sandbox of computer `id` under the gate the rebuild holds.
    fn lifecycle(&self, action: &str, id: &str) -> Result<(), String> {
        debug_assert!(runtime::operation_gate::held());
        let name = self.name_of(id)?;
        let paths = self.paths()?;
        runtime::computer_lifecycle(&paths, action, &name).map_err(|error| error.to_string())
    }

    /// Forgets what this process remembers under the names and ids of the swap.
    fn forget_state(&self, paths: &RuntimePaths, plan: &Plan) {
        for name in [&plan.old_name, &plan.undo_name, &plan.new_name] {
            runtime::forget_name_state(paths, name);
        }
        if let Some(proxy) = crate::url_proxy::shared() {
            for id in [&plan.old_id, &plan.undo_id, &plan.new_id] {
                proxy.forget_computer_id(id);
            }
        }
    }

    /// Gives the new sandbox the environment variables the old one was set up with (the
    /// creation gives it only Silo's own).
    fn copy_environment(&self, plan: &Plan) -> Result<(), String> {
        let paths = self.paths()?;
        let old = runtime::inspect_computer(&ProcessRunner, &paths, &plan.old_name)
            .map_err(|error| error.to_string())?;
        for entry in old
            .config
            .get("env")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let (Some(key), Some(value)) = (entry["key"].as_str(), entry["value"].as_str()) else {
                continue;
            };
            if key == "GH_TOKEN" {
                continue;
            }
            ProcessRunner
                .run(
                    &paths,
                    &[
                        "modify".into(),
                        plan.new_name.clone(),
                        "--env".into(),
                        format!("{key}={value}"),
                    ],
                    Duration::from_secs(60),
                )
                .map_err(|error| format!("Cannot copy the environment variable {key}: {error}"))?;
        }
        Ok(())
    }

    fn write_inventory(
        &self,
        paths: &RuntimePaths,
        change: impl FnOnce(&mut Vec<ComputerConfiguration>),
    ) -> Result<(), String> {
        let mut inventory =
            runtime::read_metadata(&paths.metadata).map_err(|error| error.to_string())?;
        let before = inventory.computers.clone();
        change(&mut inventory.computers);
        if inventory.computers == before {
            return Ok(());
        }
        runtime::write_metadata(&paths.metadata, &inventory).map_err(|error| error.to_string())
    }
}

impl RebuildEnv for EngineRebuildEnv {
    fn hold(&self, id: &str) -> Result<Box<dyn std::any::Any>, String> {
        let name = self
            .name_of(id)
            .map_err(|_| "This computer no longer exists on this device.".to_owned())?;
        let guard = runtime::OPERATIONS
            .kind(runtime::operation_gate::OperationKind::Other)
            .computer(id, &name, "Rebuilding on the newest image")
            .map_err(|error| error.to_string())?;
        Ok(Box::new(guard))
    }

    fn info(&self, id: &str) -> Result<Info, String> {
        if self.guest.os(id)? == Os::Macos {
            let (name, stopped, deleting) = macos_computers::promote_view(id)
                .ok_or("This computer no longer exists on this device.")?;
            return Ok(Info {
                id: id.into(),
                name,
                os: Os::Macos,
                container: false,
                running: !stopped,
                busy: deleting.then(|| "it is being deleted".to_owned()),
                root_disk_gib: 0,
                image: None,
            });
        }
        let paths = self.paths()?;
        let configuration = self.configuration(id)?;
        let mut image = None;
        let (running, busy) = match runtime::observe_computer(
            &ProcessRunner,
            &paths,
            configuration.name(),
        )
        .map_err(|error| error.to_string())?
        {
            runtime::ComputerRuntime::Absent => {
                (false, Some("a checkpoint restore is pending".to_owned()))
            }
            runtime::ComputerRuntime::Present(inspected) => {
                image = inspected
                    .config
                    .pointer("/image/Oci/reference")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                (
                    !matches!(inspected.status.as_str(), "Stopped" | "Created" | "Crashed"),
                    None,
                )
            }
        };
        Ok(Info {
            id: id.into(),
            name: configuration.name().to_owned(),
            os: Os::Linux,
            container: configuration.is_container(),
            running,
            busy,
            root_disk_gib: configuration.runtime_storage_gib,
            image,
        })
    }

    fn refusal(&self, info: &Info) -> Option<String> {
        let paths = self.paths().ok()?;
        if !can_clone(&paths.home) {
            return Some(
                "This device's disk cannot copy a large file by reference (it has no copy-on-write support), which moving the workspace disk needs."
                    .into(),
            );
        }
        if stored_under_its_name(&paths, &info.name) {
            return Some(format!(
                "{} keeps a disk in a folder named after it, so it cannot be rebuilt beside itself.",
                info.name
            ));
        }
        let configuration = self.configuration(&info.id).ok()?;
        match workspace_capacity_mib(&paths, &info.name) {
            Some(mib) if mib != u64::from(configuration.workspace_storage_gib) * 1024 => Some(
                format!(
                    "The workspace disk of {} does not have the size Silo has saved for it, so a new computer could not take it over.",
                    info.name
                ),
            ),
            _ => None,
        }
    }

    fn name_in_use(&self, name: &str) -> bool {
        let macos = self
            .host
            .macos_context()
            .map(|macos| macos_computers::names(&macos))
            .unwrap_or_default();
        let Ok(linux) = self.inventory() else {
            return true;
        };
        macos
            .iter()
            .map(String::as_str)
            .chain(linux.iter().map(|computer| computer.name()))
            .any(|other| other.eq_ignore_ascii_case(name))
    }

    fn recipe(&self, id: &str) -> Result<Option<RecipeSpec>, String> {
        recipes::records::get(id)
            .map(|record| record.map(|record| record.recipe.spec))
            .map_err(|error| format!("Cannot read the saved recipe records: {error}"))
    }

    fn home_bytes(&self, id: &str) -> Result<u64, String> {
        let info = self.info(id)?;
        if info.running {
            let out = checked(
                "Measuring the home folder",
                self.root_script(id, "du -sb /home/silo", &[], Duration::from_secs(120)),
            )?;
            return parse_du(&out.stdout);
        }
        // A stopped computer whose root disk holds less than the limit in total cannot hold a
        // larger home folder, so it is not started for the measurement.
        let paths = self.paths()?;
        let root = paths
            .home
            .join("sandboxes")
            .join(&info.name)
            .join("upper.ext4");
        if let Ok(meta) = fs::metadata(&root) {
            use std::os::unix::fs::MetadataExt;
            let used = meta.blocks() * 512;
            if used <= super::home_limit(info.root_disk_gib) {
                return Ok(used);
            }
        }
        self.start(id)?;
        let measured = checked(
            "Measuring the home folder",
            self.root_script(id, "du -sb /home/silo", &[], Duration::from_secs(120)),
        )
        .and_then(|out| parse_du(&out.stdout));
        let stopped = self.stop(id);
        let bytes = measured?;
        stopped?;
        Ok(bytes)
    }

    fn newest_image(&self) -> Result<String, String> {
        let paths = self.paths()?;
        ProcessRunner
            .prepare_guest_image(&paths)
            .map_err(|error| format!("Cannot prepare the newest guest image: {error}"))
    }

    fn start(&self, id: &str) -> Result<(), String> {
        self.lifecycle("start", id)
    }

    fn stop(&self, id: &str) -> Result<(), String> {
        self.lifecycle("stop", id)
    }

    fn checkpoint(&self, id: &str, name: &str) -> Result<CheckpointRef, String> {
        let paths = self.paths()?;
        let checkpoint = runtime::checkpoints::create_while_held(
            &paths,
            id,
            name,
            CheckpointKind::BeforeKeep,
            &|| Ok(0),
        )?;
        Ok(CheckpointRef {
            id: checkpoint.id,
            name: checkpoint.name,
            created_at: checkpoint.created_at,
            scope: serde_json::to_value(checkpoint.scope)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default(),
        })
    }

    fn create(&self, plan: &Plan) -> Result<(), String> {
        let mut configuration = self.configuration(&plan.old_id)?;
        configuration.id = plan.new_id.clone();
        configuration.name = plan.new_name.clone();
        runtime::change_computer_configuration_while_held(
            self.host.as_ref(),
            ComputerConfigurationChange::Upsert {
                configuration,
                expected: None,
            },
        )?;
        self.copy_environment(plan)
    }

    fn carry_home(&self, plan: &Plan) -> Result<(), String> {
        let paths = self.paths()?;
        {
            use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&self.stage_dir)
                .map_err(|error| format!("Cannot prepare the staging folder: {error}"))?;
            fs::set_permissions(&self.stage_dir, fs::Permissions::from_mode(0o700))
                .map_err(|error| format!("Cannot prepare the staging folder: {error}"))?;
        }
        let staged = Staged(self.stage_file(plan));
        let _ = fs::remove_file(&staged.0);
        let stage_path = staged.0.to_string_lossy().into_owned();
        let tar = guest_tar(plan);
        let carried = (|| -> Result<(), String> {
            let packed = checked(
                "Packing the home folder",
                self.root_script(&plan.old_id, PACK_HOME, &[&tar], TAR_TIMEOUT),
            )?;
            let report = parse_pack_report(&packed.stdout)?;
            ProcessRunner
                .run(
                    &paths,
                    &["cp".into(), format!("{}:{tar}", plan.old_name), stage_path.clone()],
                    COPY_TIMEOUT,
                )
                .map_err(|error| format!("Copying the home folder out failed: {error}"))?;
            ProcessRunner
                .run(
                    &paths,
                    &["cp".into(), stage_path.clone(), format!("{}:{tar}", plan.new_name)],
                    COPY_TIMEOUT,
                )
                .map_err(|error| format!("Copying the home folder in failed: {error}"))?;
            checked(
                "Unpacking the home folder",
                self.root_script(
                    &plan.new_id,
                    UNPACK_HOME,
                    &[&tar, &report.uid, &report.gid, &report.sum],
                    TAR_TIMEOUT,
                ),
            )?;
            Ok(())
        })();
        drop(staged);
        for id in [&plan.old_id, &plan.new_id] {
            let _ = self.root_script(id, REMOVE_TAR, &[&tar], Duration::from_secs(60));
        }
        carried
    }

    fn move_workspace(&self, plan: &Plan) -> Result<(), String> {
        debug_assert!(runtime::operation_gate::held());
        let paths = self.paths()?;
        let (old, new) = (
            workspace_capacity_mib(&paths, &plan.old_name),
            workspace_capacity_mib(&paths, &plan.new_name),
        );
        if old != new {
            return Err("The new computer's workspace disk is not the size of the old one's.".into());
        }
        let sandboxes = paths.home.join("sandboxes");
        clone_workspace(
            &sandboxes.join(&plan.old_name),
            &sandboxes.join(&plan.new_name),
        )
    }

    fn copy_ssh(&self, plan: &Plan) -> Result<(), String> {
        debug_assert!(runtime::operation_gate::held());
        let sandboxes = self.paths()?.home.join("sandboxes");
        copy_folder(
            &sandboxes.join(&plan.old_name).join("ssh"),
            &sandboxes.join(&plan.new_name).join("ssh"),
        )
    }

    fn replay_recipe(&self, id: &str, spec: &RecipeSpec) -> Result<(), String> {
        let target = self.target(id)?;
        let steps = recipes::steps_for(spec);
        replay_steps(&steps, &|line| eprintln!("rebuild: {line}"), &mut |step| {
            let out = guest::run_bounded(
                &target,
                &ExecRequest {
                    argv: vec!["bash".into(), "-c".into(), step.script.clone()],
                    cwd: None,
                    env: step
                        .args
                        .iter()
                        .map(|(name, value)| (format!("A_{name}"), value.clone()))
                        .collect(),
                    user: match step.who {
                        RunAs::Root => GuestUser::Root,
                        RunAs::Account => GuestUser::Account,
                    },
                    stdin: None,
                    timeout: STEP_TIMEOUT,
                    max_stdout: 16 * 1024 * 1024,
                },
            )
            .map_err(|error| error.message)?;
            Ok(ExecOut {
                code: out.exit_code.unwrap_or(-1),
                stdout: out.stdout,
                stderr: out.stderr,
            })
        })
    }

    fn swap_apply(&self, plan: &Plan) -> Result<(), String> {
        debug_assert!(runtime::operation_gate::held());
        let paths = self.paths()?;
        swap::apply(&RuntimeSandboxes { paths: &paths }, plan)?;
        self.write_inventory(&paths, |computers| swap::swap_inventory(computers, plan))?;
        // What the undo computer is bound by follows the computer it was: the same network
        // policy and computer-use approval, never its secrets, ports or Silo's tools.
        crate::network_policy::store::inherit(&plan.old_id, &plan.undo_id, None)?;
        crate::computer_use::inherit_settings(&paths, &plan.old_id, &plan.undo_id)
            .map_err(|error| error.to_string())?;
        // The rebuilt computer applies its approval again on its first start, and gets a token
        // of its own: the one in the old disk (now the undo computer's) stops working.
        crate::computer_use::inherit_settings(&paths, &plan.old_id, &plan.old_id)
            .map_err(|error| error.to_string())?;
        if let Some(store) = crate::guest_client::TokenStore::current() {
            if store.status(&plan.old_id)?.is_some_and(|record| record.enabled()) {
                store.mint(&plan.old_id)?;
            }
        }
        self.forget_state(&paths, plan);
        Ok(())
    }

    fn swap_revert(&self, plan: &Plan) -> Result<(), String> {
        debug_assert!(runtime::operation_gate::held());
        let paths = self.paths()?;
        self.write_inventory(&paths, |computers| swap::unswap_inventory(computers, plan))?;
        swap::revert(&RuntimeSandboxes { paths: &paths }, plan)?;
        crate::network_policy::computer_removed(&plan.undo_id);
        let _ = crate::computer_use::forget(&paths, &plan.undo_id);
        self.forget_state(&paths, plan);
        Ok(())
    }

    fn settle_swapped(&self, plan: &Plan) -> Result<(), String> {
        let paths = self.paths()?;
        // The undo computer has the recipe record its computer had.
        recipes::records::inherit(&plan.old_id, &plan.undo_id)?;
        // What the build was given under its own id and name goes; the sandbox lives on as
        // the computer.
        let mut gone = self.configuration(&plan.old_id)?;
        gone.id = plan.new_id.clone();
        gone.name = plan.new_name.clone();
        runtime::forget_replaced_identity(&paths, &gone).map_err(|error| error.to_string())?;
        // The blank workspace disk the clone replaced is of no use to anyone.
        let _ = fs::remove_dir_all(
            paths
                .home
                .join("sandboxes")
                .join(&plan.old_name)
                .join(".workspace-before-swap"),
        );
        runtime::publish_computers_changed(self.host.as_ref(), &paths);
        Ok(())
    }

    fn remove_build(&self, plan: &Plan) -> Result<(), String> {
        let _ = fs::remove_file(self.stage_file(plan));
        let paths = self.paths()?;
        match self
            .inventory()?
            .into_iter()
            .find(|computer| computer.id() == plan.new_id)
        {
            Some(expected) => {
                runtime::change_computer_configuration_while_held(
                    self.host.as_ref(),
                    ComputerConfigurationChange::Delete {
                        computer_id: plan.new_id.clone(),
                        expected,
                    },
                )?;
            }
            None => {
                // A creation that failed before the computer was saved may have left a sandbox.
                runtime::cleanup_failed_create(
                    &ProcessRunner,
                    &paths,
                    &plan.new_name,
                    &plan.new_id,
                )
                .map_err(|error| error.to_string())?;
            }
        }
        runtime::publish_computers_changed(self.host.as_ref(), &paths);
        Ok(())
    }
}
