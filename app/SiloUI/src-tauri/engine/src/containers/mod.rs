//! Container computers: Linux computers that run under rootless crun instead of MicroSandbox.
//!
//! The rest of the engine asks the runtime for computers in MicroSandbox's argument shape
//! (`create`, `inspect`, `start`, `stop`, `exec`, `remove`, `list`). The one place that spawns
//! the runtime, `run_msb_process`, calls [`route`] first: a command that names a container
//! computer (or creates one) is answered here by [`run`], and a `list` is completed with the
//! container computers by [`merge_list`]. Everything above that point (locks, the working
//! account, change capture, the API) is shared with virtual machines. Streaming guest commands
//! are built by [`guest_plan`].
//!
//! Layout and design: [`layout`], [`oci`] (the hardening baseline), [`image`] (the pinned image,
//! unpacked once per digest) and [`lifecycle`] (the holder, the gateway and `crun`).

pub mod archive;
pub mod args;
pub mod capability;
pub mod cli;
pub mod desktop;
pub mod hold;
pub mod host_disk;
pub mod idmap;
pub mod image;
pub mod layers;
pub mod layout;
#[cfg(target_os = "linux")]
pub mod lifecycle;
pub mod oci;
#[cfg(target_os = "linux")]
pub mod ports;
pub mod secrets;
pub mod snapshots;
pub mod ssh;
pub mod tools;
pub mod users;

pub use args::RUNTIME_LABEL;

/// Whether a computer's runtime configuration (the `inspect` answer an export records) is a
/// container computer's: it carries the label its creation sets.
pub fn is_container_config(config: &Value) -> bool {
    RUNTIME_LABEL.split_once('=').is_some_and(|(key, value)| {
        config
            .get("labels")
            .and_then(|labels| labels.get(key))
            .and_then(Value::as_str)
            == Some(value)
    })
}

/// The program and leading argument of the container runtime command (`cli`): `silod`, which
/// is installed beside the bundled `msb`.
pub fn runtime_invocation(paths: &RuntimePaths) -> (PathBuf, Vec<String>) {
    (
        paths.executable.with_file_name("silod"),
        vec![cli::FLAG.to_owned()],
    )
}

/// The secret material one runtime command may use: the values of the general secrets assigned
/// to the computer (name, value, allowed hosts) and the GitHub access profile. It is read from
/// the host credential store by the caller, handed to the gateway and never kept or logged.
pub struct Material<'a> {
    pub secrets: &'a [(String, String, Vec<String>)],
    pub github_profile: &'a str,
}

impl Material<'static> {
    /// No secrets and GitHub access off.
    pub const NONE: Material<'static> = Material {
        secrets: &[],
        github_profile: crate::runtime::DISABLED_GITHUB_PROFILE,
    };
}

use crate::runtime::{CommandOutput, RuntimeError, RuntimePaths};
use args::{Command, Create};
use layout::{ComputerRecord, Layout, RECORD_VERSION};
use serde_json::{json, Value};
use std::{path::PathBuf, time::Duration};

/// The quiet step (`runtime/checkpoints/quiet.sh`) for a container. A container's host name is
/// set when it starts and its `/etc/hosts` is a file bound from outside that its root cannot write,
/// so those two steps are allowed to fail; everything else (identity, scheduled jobs, services,
/// scrubs) is the virtual machine's script unchanged.
pub fn quiet_script(script: &str) -> String {
    script
        .replacen(
            "hostname \"$host\" || exit 1",
            "hostname \"$host\" || true",
            1,
        )
        .replacen(
            "cat \"$quiet/hosts.new\" > /etc/hosts || exit 1",
            "cat \"$quiet/hosts.new\" > /etc/hosts || true",
            1,
        )
}

/// Once, when the engine starts: stops the container computers an earlier engine process left
/// running (their gateways went with it). Reading a computer's state never does this.
pub fn reconcile_at_start(paths: &RuntimePaths) {
    #[cfg(target_os = "linux")]
    {
        if !layout::Layout::new(&paths.home).names().is_empty() {
            if let Ok(tools) = tools::Tools::locate(paths) {
                lifecycle::reconcile(paths, &tools);
            }
        }
        // Looks at the host's free space for as long as the engine runs (see `host_disk`).
        lifecycle::spawn_guard(paths);
    }
    #[cfg(not(target_os = "linux"))]
    let _ = paths;
}

/// The reference the runtime is told to create from; the container runtime unpacks the pinned
/// image itself.
pub fn guest_image_reference() -> Result<String, RuntimeError> {
    image::pinned().map(|manifest| manifest.image_reference)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    /// MicroSandbox answers.
    Runtime,
    /// A container computer answers.
    Container,
    /// MicroSandbox answers and the container computers are added to its list.
    MergeList,
}

pub fn route(paths: &RuntimePaths, args: &[String]) -> Route {
    if args::creates_container(args) {
        return Route::Container;
    }
    let layout = Layout::new(&paths.home);
    if args.first().map(String::as_str) == Some("list") {
        return if layout.names().is_empty() {
            Route::Runtime
        } else {
            Route::MergeList
        };
    }
    let snapshots = layers::Store::new(&layout);
    if args.first().map(String::as_str) == Some("snapshot") {
        match args.get(1).map(String::as_str) {
            // A capture is routed by the computer it is taken from.
            Some("create") => {}
            // A file does not say which runtime wrote it: the caller does.
            Some("load") => {
                return if args::loads_container(args) {
                    Route::Container
                } else {
                    Route::Runtime
                }
            }
            // Without a group the answer is MicroSandbox's with the container checkpoints added.
            Some("list") if args::snapshot_group(args).is_none() => {
                // An imported container has checkpoints before it has a record.
                return if layout.names().is_empty() && !layout.snapshots().is_dir() {
                    Route::Runtime
                } else {
                    Route::MergeList
                };
            }
            _ => {
                return match args::snapshot_group(args) {
                    Some(group) if snapshots.group_exists(group) => Route::Container,
                    _ => Route::Runtime,
                }
            }
        }
    }
    if args.first().map(String::as_str) == Some("restore") {
        // The new computer does not exist yet: the checkpoint's group says whose it is.
        return match args::snapshot_group(args) {
            Some(group) if snapshots.group_exists(group) => Route::Container,
            _ => Route::Runtime,
        };
    }
    match args::target_name(args) {
        Some(name)
            if crate::runtime::validate_name(name).is_ok() && layout.computer(name).exists() =>
        {
            Route::Container
        }
        _ => Route::Runtime,
    }
}

fn require_host() -> Result<(), RuntimeError> {
    match silo_host::container_prerequisites::check().refusal() {
        None => Ok(()),
        Some(message) => Err(RuntimeError::Unavailable(message)),
    }
}

fn not_yet(verb: &str) -> RuntimeError {
    RuntimeError::Invalid(capability::refusal(verb))
}

fn output(stdout: String) -> CommandOutput {
    CommandOutput {
        stdout,
        stderr: String::new(),
    }
}

pub fn run(
    paths: &RuntimePaths,
    args: &[String],
    timeout: Duration,
    material: &Material<'_>,
) -> Result<CommandOutput, RuntimeError> {
    let command = args::parse(args).map_err(RuntimeError::Invalid)?;
    match command {
        Command::Create(create) => create_computer(paths, &create).map(|()| output(String::new())),
        Command::Inspect { name } => inspect(paths, &name).map(|value| output(value.to_string())),
        Command::List { .. } => Ok(output(Value::Array(listed(paths, None)).to_string())),
        Command::Unsupported { verb } => Err(not_yet(&verb)),
        Command::SnapshotCreate(request) => {
            snapshots::create(paths, &request).map(|()| output(String::new()))
        }
        Command::SnapshotList { group } => snapshots::list(paths, group.as_deref())
            .map(|rows| output(Value::Array(rows).to_string())),
        Command::SnapshotVerify { group, name } => {
            snapshots::verify(paths, &group, &name).map(|()| output(String::new()))
        }
        Command::SnapshotRemove { group, name } => {
            snapshots::remove(paths, &group, &name)?;
            collect_layers(paths);
            Ok(output(String::new()))
        }
        Command::SnapshotHead { group } => {
            snapshots::head(paths, &group).map(|value| output(value.to_string()))
        }
        Command::SnapshotSave(save) => archive::save(paths, &save).map(|()| output(String::new())),
        Command::SnapshotLoad(load) => archive::load(paths, &load).map(|()| output(String::new())),
        Command::Restore(restore) => {
            snapshots::make_fork(paths, &restore)?;
            start(paths, &restore.name, timeout, material).map(|()| output(String::new()))
        }
        Command::Pause { name } => freeze(paths, &name, true).map(|()| output(String::new())),
        Command::Resume { name } => freeze(paths, &name, false).map(|()| output(String::new())),
        Command::SshServe(_) => Err(RuntimeError::Invalid(
            "ssh serve is a long-running command, started through the container runtime command."
                .into(),
        )),
        Command::Start { name } => {
            start(paths, &name, timeout, material).map(|()| output(String::new()))
        }
        Command::Modify(modify) => {
            modify_computer(paths, &modify, material).map(|()| output(String::new()))
        }
        Command::Stop { name } => stop(paths, &name).map(|()| output(String::new())),
        Command::Remove { name } => remove(paths, &name).map(|()| output(String::new())),
        Command::Exec(exec) => exec_and_wait(paths, &exec, timeout),
    }
}

fn record_of(paths: &RuntimePaths, name: &str) -> Result<ComputerRecord, RuntimeError> {
    Layout::new(&paths.home)
        .computer(name)
        .read_record()
        .map_err(|_| RuntimeError::Failed {
            operation: "Reading computer state".into(),
            exit_code: Some(1),
            detail: format!("sandbox {name} not found"),
        })
}

fn create_computer(paths: &RuntimePaths, create: &Create) -> Result<(), RuntimeError> {
    crate::runtime::validate_name(&create.name)?;
    require_host()?;
    let layout = Layout::new(&paths.home);
    let dir = layout.computer(&create.name);
    if dir.exists() {
        return Err(RuntimeError::Invalid(format!(
            "Computer '{}' already exists in the runtime. No existing computer was changed.",
            create.name
        )));
    }
    #[cfg(target_os = "linux")]
    let image = {
        let tools = tools::Tools::locate(paths)?;
        image::ensure(paths, &tools)?
    };
    #[cfg(not(target_os = "linux"))]
    let image = image::Unpacked {
        rootfs: PathBuf::new(),
        digest: image::pinned()?.image_digest,
        reference: image::pinned()?.image_reference,
    };
    let failed = |error: String| RuntimeError::Unavailable(error);
    std::fs::create_dir_all(dir.path().join("layers")).map_err(|e| failed(e.to_string()))?;
    let record = new_record(create, &image);
    dir.write_record(&record).map_err(failed)
}

/// Records a secret change and applies it to the running gateway, if there is one. The gateway
/// is updated first, so a removal takes effect before it is written down; any failed exchange
/// has already stopped the gateway (it fails closed), so a change that is not confirmed is
/// never left looking applied.
fn modify_computer(
    paths: &RuntimePaths,
    modify: &args::Modify,
    material: &Material<'_>,
) -> Result<(), RuntimeError> {
    let dir = Layout::new(&paths.home).computer(&modify.name);
    let before = record_of(paths, &modify.name)?;
    // The change is checked before anything is applied; it is written below, to the record as it
    // is then, not to a copy read before the gateway exchange.
    secrets::apply_to_record(&mut before.clone(), modify).map_err(RuntimeError::Invalid)?;
    #[cfg(target_os = "linux")]
    live_update(paths, &before, modify, material)?;
    #[cfg(not(target_os = "linux"))]
    let _ = material;
    record_change(&dir, modify)
}

/// Writes a `modify` into the record as it is now, so a change another request made while this
/// one waited on the gateway is kept.
fn record_change(dir: &layout::ComputerDir, modify: &args::Modify) -> Result<(), RuntimeError> {
    let mut refused = None;
    dir.update_record(|record| {
        refused = secrets::apply_to_record(record, modify).err();
    })
    .map_err(RuntimeError::Unavailable)?;
    // The check depends on the change alone, so it passed before; were it ever to fail the record
    // would hold a half-applied change, so it is reported rather than hidden.
    match refused {
        Some(detail) => Err(RuntimeError::Invalid(detail)),
        None => Ok(()),
    }
}

/// Why a change to the running gateway did not apply.
#[cfg(target_os = "linux")]
enum LiveFailure {
    /// Refused before the gateway was asked: the gateway is as it was.
    Refused(RuntimeError),
    /// An exchange with the gateway failed. The gateway fails closed: it has stopped.
    Exchange(String),
}

#[cfg(target_os = "linux")]
fn live_update(
    paths: &RuntimePaths,
    before: &ComputerRecord,
    modify: &args::Modify,
    material: &Material<'_>,
) -> Result<(), RuntimeError> {
    let name = modify.name.as_str();
    if lifecycle::status(paths, name, before) != lifecycle::Status::Running {
        return Ok(());
    }
    let Some(running) = lifecycle::running(name) else {
        return Ok(());
    };
    match apply_live(&running, before, modify, material) {
        Ok(()) => Ok(()),
        Err(LiveFailure::Refused(error)) => Err(error),
        Err(LiveFailure::Exchange(detail)) => {
            let tools = tools::Tools::locate(paths).ok();
            Err(stopped_after_failed_exchange(
                paths,
                tools.as_ref(),
                before,
                detail,
            ))
        }
    }
}

/// A gateway that failed an exchange has stopped and the computer has no network, but its
/// processes run on: the computer is stopped too, so what the disk and the interface say matches
/// what is true. The error names both.
#[cfg(target_os = "linux")]
fn stopped_after_failed_exchange(
    paths: &RuntimePaths,
    tools: Option<&tools::Tools>,
    record: &ComputerRecord,
    detail: String,
) -> RuntimeError {
    let stopped = tools.map(|tools| lifecycle::stop(paths, tools, &record.name, record));
    eprintln!("Updating container computer {}: {detail}", record.name);
    RuntimeError::Unavailable(match stopped {
        Some(Ok(())) => format!(
            "Silo could not update the network gateway of container computer {}, so it stopped the computer. Start it again.",
            record.name
        ),
        _ => format!(
            "Silo could not update the network gateway of container computer {}, and could not stop the computer. Stop it before using it.",
            record.name
        ),
    })
}

#[cfg(target_os = "linux")]
fn apply_live(
    running: &lifecycle::Running,
    before: &ComputerRecord,
    modify: &args::Modify,
    material: &Material<'_>,
) -> Result<(), LiveFailure> {
    use crate::computer_gateway::Secret;
    use zeroize::Zeroizing;
    let mut active = running.active_secrets();
    let mut gateway = running.gateway();
    for spec in modify.put.iter().filter(|_| !modify.next_start) {
        if spec.name == args::GITHUB_SECRET_NAME {
            if !before.github {
                return Err(LiveFailure::Refused(RuntimeError::Invalid(
                    "This computer was started without GitHub access. Restart it to turn GitHub access on."
                        .into(),
                )));
            }
            gateway
                .set_github_profile(Some(material.github_profile))
                .map_err(LiveFailure::Exchange)?;
            continue;
        }
        let Some((_, value, hosts)) = material
            .secrets
            .iter()
            .find(|(secret, _, _)| secret == &spec.name)
        else {
            return Err(LiveFailure::Refused(RuntimeError::Invalid(format!(
                "The value of secret {} is not available to the engine.",
                spec.name
            ))));
        };
        gateway
            .put_secret(&Secret {
                name: spec.name.clone(),
                value: Zeroizing::new(value.clone()),
                allowed_hosts: hosts.clone(),
            })
            .map_err(LiveFailure::Exchange)?;
        if !active.contains(&spec.name) {
            active.push(spec.name.clone());
        }
        running.set_active(active.clone());
    }
    for removed in &modify.remove {
        // The gateway refuses to remove a secret it does not hold, which would stop it.
        if active.contains(removed) {
            gateway
                .remove_secret(removed)
                .map_err(LiveFailure::Exchange)?;
            active.retain(|name| name != removed);
            running.set_active(active.clone());
        }
    }
    Ok(())
}

fn new_record(create: &Create, image: &image::Unpacked) -> ComputerRecord {
    ComputerRecord {
        schema_version: RECORD_VERSION,
        name: create.name.clone(),
        cpus: create.cpus,
        max_cpus: create.max_cpus,
        memory_mib: create.memory_mib,
        max_memory_mib: create.max_memory_mib,
        root_disk_mib: create.root_disk_mib,
        workspace_mib: create.workspace_mib,
        labels: create.labels.clone(),
        env: create.env.clone(),
        image_digest: image.digest.clone(),
        image_reference: image.reference.clone(),
        layers: Vec::new(),
        upper: layers::new_id(),
        created_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
        started_once: false,
        secrets: Vec::new(),
        github: create.github,
        network_blocked: false,
    }
}

/// The state a read reports. It only looks: a list or an inspect never changes a computer.
#[cfg(target_os = "linux")]
fn state_word(paths: &RuntimePaths, name: &str, record: &ComputerRecord) -> &'static str {
    match lifecycle::status(paths, name, record) {
        lifecycle::Status::Running if lifecycle::is_paused(paths, name) => "Paused",
        lifecycle::Status::Running => "Running",
        lifecycle::Status::Starting => "Starting",
        lifecycle::Status::Stopped => "Stopped",
        lifecycle::Status::Created => "Created",
    }
}

#[cfg(not(target_os = "linux"))]
fn state_word(_: &RuntimePaths, _: &str, record: &ComputerRecord) -> &'static str {
    stopped_word(record)
}

#[cfg(not(target_os = "linux"))]
fn stopped_word(record: &ComputerRecord) -> &'static str {
    if record.started_once {
        "Stopped"
    } else {
        "Created"
    }
}

/// `inspect` in the shape the engine reads: the saved configuration the engine compares with its
/// own (resources, root disk, workspace disk, labels) and, while running, the same as active.
pub fn inspect_value(
    name: &str,
    record: &ComputerRecord,
    status: &str,
    active: &[String],
) -> Value {
    let labels: serde_json::Map<String, Value> = record
        .labels
        .iter()
        .map(|(key, value)| (key.clone(), Value::String(value.clone())))
        .collect();
    let config = json!({
        "name": name,
        "labels": labels,
        "resources": {
            "cpus": record.cpus,
            "max_cpus": record.max_cpus,
            "memory_mib": record.memory_mib,
            "max_memory_mib": record.max_memory_mib,
        },
        "image": {"Oci": {"reference": record.image_reference, "root_disk": {"kind": "managed", "size_mib": record.root_disk_mib}}},
        "mounts": [{
            "guest": crate::runtime::WORKSPACE_MOUNT,
            "type": "Owned",
            "storage": {"kind": "disk", "capacity_mib": record.workspace_mib},
        }],
        "network": secrets::network_value(record, None),
    });
    let mut value = json!({
        "name": name,
        "status": status,
        "config": config,
        "updated_at": null,
        "runtime_instance_id": null,
    });
    if status == "Running" || status == "Paused" {
        value["active_config"] = value["config"].clone();
        value["active_config"]["network"] = secrets::network_value(record, Some(active));
        value["runtime_instance_id"] = json!(format!("container-{name}-{}", record.created_ms));
    }
    value
}

fn inspect(paths: &RuntimePaths, name: &str) -> Result<Value, RuntimeError> {
    let record = record_of(paths, name)?;
    #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
    let mut value = inspect_value(
        name,
        &record,
        state_word(paths, name, &record),
        &active_secrets(name),
    );
    // Why Silo stopped the computer, when the host disk guard did (see `host_disk`).
    #[cfg(target_os = "linux")]
    if let Some(notice) = lifecycle::guard_notice(paths, name) {
        value["notice"] = json!(notice);
    }
    Ok(value)
}

fn listed(paths: &RuntimePaths, label: Option<&str>) -> Vec<Value> {
    let layout = Layout::new(&paths.home);
    layout
        .names()
        .into_iter()
        .filter_map(|name| {
            let record = layout.computer(&name).read_record().ok()?;
            let matches = label.is_none_or(|wanted| {
                wanted
                    .split_once('=')
                    .is_some_and(|(key, value)| record.label(key) == Some(value))
            });
            matches.then(|| json!({"name": name, "status": state_word(paths, &name, &record)}))
        })
        .collect()
}

/// MicroSandbox's `list` answer (of computers or of checkpoints) with the container ones added.
pub fn merge_list(
    paths: &RuntimePaths,
    args: &[String],
    answer: CommandOutput,
) -> Result<CommandOutput, RuntimeError> {
    if args.first().map(String::as_str) == Some("snapshot") {
        return snapshots::merge_list(paths, args, answer);
    }
    let mut rows: Vec<Value> = serde_json::from_str(&answer.stdout).map_err(|_| {
        RuntimeError::Malformed("The bundled runtime returned an invalid computer list.".into())
    })?;
    let label = args
        .windows(2)
        .find(|pair| pair[0] == "--label")
        .map(|pair| pair[1].as_str());
    rows.extend(listed(paths, label));
    Ok(CommandOutput {
        stdout: Value::Array(rows).to_string(),
        stderr: answer.stderr,
    })
}

#[cfg(target_os = "linux")]
fn active_secrets(name: &str) -> Vec<String> {
    lifecycle::active_secrets(name)
}

#[cfg(not(target_os = "linux"))]
fn active_secrets(_: &str) -> Vec<String> {
    Vec::new()
}

#[cfg(target_os = "linux")]
fn start(
    paths: &RuntimePaths,
    name: &str,
    timeout: Duration,
    material: &Material<'_>,
) -> Result<(), RuntimeError> {
    require_host()?;
    let tools = tools::Tools::locate(paths)?;
    let running_before =
        lifecycle::status(paths, name, &record_of(paths, name)?) == lifecycle::Status::Running;
    lifecycle::start(paths, &tools, name, timeout, material)?;
    if running_before {
        return Ok(());
    }
    // A computer that cannot trust its gateway's certificates would fail every request to a host
    // a secret is allowed for, so it does not stay up.
    install_gateway_ca(paths, name).map_err(|error| {
        eprintln!("Starting container computer {name}: the gateway certificate: {error:?}");
        let _ = stop(paths, name);
        RuntimeError::Unavailable(format!(
            "Silo could not add its network gateway's certificate to the trust store of container computer {name}, so it did not start. The image needs update-ca-certificates."
        ))
    })?;
    start_desktop(paths, name);
    Ok(())
}

/// Starts the computer's desktop the way a virtual machine's boot does, in the background: the
/// start does not wait for it, and a desktop that does not start never fails the computer (the
/// desktop's status says `failed`, see [`desktop::boot_command`]). The command is bounded by
/// [`desktop::BOOT_TIMEOUT`], and it ends sooner if the computer stops.
#[cfg(target_os = "linux")]
fn start_desktop(paths: &RuntimePaths, name: &str) {
    let paths = paths.clone();
    let owned = name.to_owned();
    let spawned = std::thread::Builder::new()
        .name(format!("desktop-boot-{name}"))
        .spawn(move || {
            let name = owned;
            let command = desktop::boot_command(&name);
            if let Err(error) = exec_and_wait(
                &paths,
                &command,
                desktop::BOOT_TIMEOUT + Duration::from_secs(10),
            ) {
                eprintln!("Container computer {name}: the desktop boot: {error:?}");
            }
        });
    if let Err(error) = spawned {
        eprintln!("Container computer {name}: the desktop boot could not start: {error}");
    }
}

/// The root command that adds the gateway CA (bound at [`oci::GATEWAY_CA_PATH`]) to the guest's
/// trust bundle. `--fresh` rebuilds the certificate links, so a CA an earlier start left in the
/// computer's own files is dropped when it is no longer the one bound.
#[cfg(target_os = "linux")]
fn gateway_ca_command(name: &str) -> args::Exec {
    args::Exec {
        name: name.to_owned(),
        user: Some("root".into()),
        env: Vec::new(),
        fixed_environment: true,
        workdir: Some("/".into()),
        no_start: true,
        timeout: Some(GATEWAY_CA_TIMEOUT),
        argv: vec!["update-ca-certificates".into(), "--fresh".into()],
        ..args::Exec::default()
    }
}

/// How long `update-ca-certificates` may take, hooks (a Java trust store, say) included.
#[cfg(target_os = "linux")]
const GATEWAY_CA_TIMEOUT: Duration = Duration::from_secs(120);

#[cfg(target_os = "linux")]
fn install_gateway_ca(paths: &RuntimePaths, name: &str) -> Result<(), RuntimeError> {
    exec_and_wait(
        paths,
        &gateway_ca_command(name),
        GATEWAY_CA_TIMEOUT + Duration::from_secs(10),
    )
    .map(|_| ())
}

#[cfg(target_os = "linux")]
fn stop(paths: &RuntimePaths, name: &str) -> Result<(), RuntimeError> {
    let record = record_of(paths, name)?;
    let tools = tools::Tools::locate(paths)?;
    lifecycle::stop(paths, &tools, name, &record)
}

#[cfg(target_os = "linux")]
fn freeze(paths: &RuntimePaths, name: &str, freeze: bool) -> Result<(), RuntimeError> {
    require_host()?;
    if freeze {
        lifecycle::pause(paths, name)
    } else {
        lifecycle::resume(paths, name)
    }
    .map_err(RuntimeError::Unavailable)
}

#[cfg(target_os = "linux")]
fn collect_layers(paths: &RuntimePaths) {
    if let Ok(tools) = tools::Tools::locate(paths) {
        lifecycle::collect_layers(paths, &tools);
    }
}

#[cfg(not(target_os = "linux"))]
fn freeze(_: &RuntimePaths, _: &str, _: bool) -> Result<(), RuntimeError> {
    require_host()
}

#[cfg(not(target_os = "linux"))]
fn collect_layers(_: &RuntimePaths) {}

#[cfg(target_os = "linux")]
fn remove(paths: &RuntimePaths, name: &str) -> Result<(), RuntimeError> {
    let tools = tools::Tools::locate(paths)?;
    lifecycle::remove(paths, &tools, name)
}

#[cfg(not(target_os = "linux"))]
fn start(_: &RuntimePaths, _: &str, _: Duration, _: &Material<'_>) -> Result<(), RuntimeError> {
    require_host()
}

#[cfg(not(target_os = "linux"))]
fn stop(_: &RuntimePaths, _: &str) -> Result<(), RuntimeError> {
    require_host()
}

#[cfg(not(target_os = "linux"))]
fn remove(paths: &RuntimePaths, name: &str) -> Result<(), RuntimeError> {
    let _ = std::fs::remove_dir_all(Layout::new(&paths.home).computer(name).path());
    Ok(())
}

/// Collects a child's output stream on its own thread, up to 64 MiB.
#[cfg(target_os = "linux")]
fn read_to_string<R: std::io::Read + Send + 'static>(
    stream: Option<R>,
) -> std::thread::JoinHandle<String> {
    use std::io::Read;
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(stream) = stream {
            let _ = stream.take(64 * 1024 * 1024).read_to_end(&mut bytes);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

#[cfg(target_os = "linux")]
fn exec_and_wait(
    paths: &RuntimePaths,
    exec: &args::Exec,
    timeout: Duration,
) -> Result<CommandOutput, RuntimeError> {
    use std::process::Stdio;
    let record = record_of(paths, &exec.name)?;
    let tools = tools::Tools::locate(paths)?;
    if lifecycle::status(paths, &exec.name, &record) != lifecycle::Status::Running {
        return Err(RuntimeError::Unavailable(format!(
            "Start {} first.",
            exec.name
        )));
    }
    let lifecycle::ExecPlan { mut command, group } =
        lifecycle::exec_command(paths, &tools, &exec.name, exec, true)
            .map_err(RuntimeError::Invalid)?;
    let operation = "Running a command in the computer".to_owned();
    // The crun process is only the carrier: when it is killed the command must be too.
    let end = |child: &mut std::process::Child| {
        group.kill();
        let _ = child.kill();
        let _ = child.wait();
    };
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| RuntimeError::Launch(error.to_string()))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let (out, err) = (read_to_string(stdout), read_to_string(stderr));
    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20))
            }
            Ok(None) => {
                end(&mut child);
                return Err(RuntimeError::TimedOut { operation });
            }
            Err(error) => {
                end(&mut child);
                return Err(RuntimeError::Failed {
                    operation,
                    exit_code: None,
                    detail: error.to_string(),
                });
            }
        }
    };
    let (stdout, stderr) = (
        out.join().unwrap_or_default(),
        err.join().unwrap_or_default(),
    );
    match status.code() {
        Some(0) => Ok(CommandOutput { stdout, stderr }),
        Some(124) if exec.timeout.is_some() => Err(RuntimeError::TimedOut { operation }),
        code => Err(RuntimeError::Failed {
            operation,
            exit_code: code,
            detail: if stderr.trim().is_empty() {
                stdout
            } else {
                stderr
            },
        }),
    }
}

#[cfg(not(target_os = "linux"))]
fn exec_and_wait(
    _: &RuntimePaths,
    _: &args::Exec,
    _: Duration,
) -> Result<CommandOutput, RuntimeError> {
    require_host().and(Err(not_yet("exec")))
}

/// The program and arguments that run `argv` in container computer `name`, for the streaming
/// guest commands that start the process themselves. The returned [`hold::ExecGroup`] owns the
/// files the arguments name; keep it until the process has ended.
pub fn guest_plan(
    paths: &RuntimePaths,
    name: &str,
    user: &str,
    argv: &[String],
    env: &[(String, String)],
) -> Result<(PathBuf, Vec<String>, hold::ExecGroup), String> {
    #[cfg(target_os = "linux")]
    {
        let tools = tools::Tools::locate(paths).map_err(|e| e.to_string())?;
        let exec = args::Exec {
            name: name.to_owned(),
            user: Some(user.to_owned()),
            env: env.to_vec(),
            workdir: None,
            no_start: true,
            timeout: None,
            argv: argv.to_vec(),
            ..args::Exec::default()
        };
        let lifecycle::ExecPlan { command, group } =
            lifecycle::exec_command(paths, &tools, name, &exec, false)?;
        Ok((
            PathBuf::from(command.get_program()),
            command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
            group,
        ))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (paths, name, user, argv, env);
        Err("Container computers run only on a Linux host.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(home: &std::path::Path) -> RuntimePaths {
        RuntimePaths {
            guest_image: home.join("g"),
            executable: home.join("msb"),
            home: home.to_path_buf(),
            storage_home: None,
            library: home.join("lib"),
            metadata: home.join("metadata"),
            volumes: home.join("volumes"),
        }
    }

    fn strings(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_owned()).collect()
    }

    fn make(home: &std::path::Path, name: &str, started: bool) {
        let create = Create {
            name: name.into(),
            cpus: 2,
            max_cpus: 2,
            memory_mib: 2048,
            max_memory_mib: 2048,
            root_disk_mib: 4096,
            workspace_mib: 1024,
            labels: vec![
                ("silo.managed".into(), "true".into()),
                ("silo.machine-id".into(), "id1".into()),
            ],
            env: Vec::new(),
            github: false,
        };
        let image = image::Unpacked {
            rootfs: PathBuf::new(),
            digest: "sha256:d".into(),
            reference: "r:1".into(),
        };
        let layout = Layout::new(home);
        let dir = layout.computer(name);
        std::fs::create_dir_all(dir.path()).unwrap();
        let mut record = new_record(&create, &image);
        record.started_once = started;
        dir.write_record(&record).unwrap();
    }

    fn modification(name: &str, put: &[&str], remove: &[&str]) -> args::Modify {
        args::Modify {
            name: name.into(),
            put: put
                .iter()
                .map(|secret| args::SecretSpec {
                    name: (*secret).into(),
                    allowed_hosts: vec!["api.example.test".into()],
                })
                .collect(),
            remove: remove.iter().map(|secret| (*secret).to_owned()).collect(),
            next_start: false,
        }
    }

    #[test]
    fn a_change_is_written_to_the_record_as_it_is_when_the_exchange_is_over() {
        let home = tempfile::tempdir().unwrap();
        make(home.path(), "box", true);
        let dir = Layout::new(home.path()).computer("box");
        let before = dir.read_record().unwrap();
        // Another request assigns a secret while this one waits on the gateway.
        dir.update_record(|record| {
            record.secrets.push(layout::SecretRecord {
                name: "OTHER".into(),
                allowed_hosts: vec!["other.example.test".into()],
            });
            record.started_once = true;
        })
        .unwrap();
        record_change(&dir, &modification("box", &["MINE"], &[])).unwrap();
        let after = dir.read_record().unwrap();
        let names: Vec<&str> = after.secrets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            ["MINE", "OTHER"],
            "the concurrent assignment was lost"
        );
        assert!(before.secrets.is_empty());
        // A removal takes only what it names.
        record_change(&dir, &modification("box", &[], &["MINE"])).unwrap();
        let names: Vec<String> = dir
            .read_record()
            .unwrap()
            .secrets
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, ["OTHER"]);
    }

    #[cfg(target_os = "linux")]
    fn unreachable_tools(directory: &std::path::Path) -> tools::Tools {
        let missing = |name: &str| directory.join(name);
        tools::Tools {
            ctr: missing("silo-ctr"),
            init: missing("silo-init"),
            gateway: missing("gateway"),
            crun: missing("crun"),
            skopeo: missing("skopeo"),
            umoci: missing("umoci"),
            systemd_run: missing("systemd-run"),
            systemctl: missing("systemctl"),
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_failed_gateway_exchange_stops_the_computer_instead_of_leaving_it_looking_stopped() {
        use std::os::unix::net::UnixStream;
        let home = tempfile::tempdir().unwrap();
        let paths = crate::test_support::paths(home.path());
        make(&paths.home, "exchange-fails", true);
        let record = record_of(&paths, "exchange-fails").unwrap();
        // The gateway's control link is gone: its next exchange fails.
        let (control, peer) = UnixStream::pair().unwrap();
        drop(peer);
        let running = lifecycle::register_for_test(
            "exchange-fails",
            crate::computer_gateway::Gateway::for_test(control),
            vec!["TOKEN".into()],
        );
        let modify = modification("exchange-fails", &[], &["TOKEN"]);
        let failure = apply_live(&running, &record, &modify, &Material::NONE)
            .err()
            .expect("the exchange should fail");
        let LiveFailure::Exchange(detail) = failure else {
            panic!("a refused exchange is not a failed one");
        };
        // The computer is still registered as running until the failure is handled.
        assert!(lifecycle::running("exchange-fails").is_some());
        let tools = unreachable_tools(home.path());
        let error = stopped_after_failed_exchange(&paths, Some(&tools), &record, detail);
        match error {
            RuntimeError::Unavailable(message) => {
                assert!(message.contains("stopped the computer"), "{message}");
                assert!(!message.contains("could not stop"), "{message}");
            }
            other => panic!("{other:?}"),
        }
        // Stopped: no gateway is registered, so the computer reads as stopped on disk and in the UI.
        assert!(lifecycle::running("exchange-fails").is_none());
        assert!(lifecycle::active_secrets("exchange-fails").is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_change_refused_before_the_exchange_leaves_the_computer_running() {
        use std::os::unix::net::UnixStream;
        let home = tempfile::tempdir().unwrap();
        let paths = crate::test_support::paths(home.path());
        make(&paths.home, "refused", true);
        let record = record_of(&paths, "refused").unwrap();
        let (control, _peer) = UnixStream::pair().unwrap();
        let running = lifecycle::register_for_test(
            "refused",
            crate::computer_gateway::Gateway::for_test(control),
            Vec::new(),
        );
        // The engine holds no value for the secret: nothing is asked of the gateway.
        let modify = modification("refused", &["MISSING"], &[]);
        let failure = apply_live(&running, &record, &modify, &Material::NONE)
            .err()
            .expect("the change should be refused");
        assert!(matches!(
            failure,
            LiveFailure::Refused(RuntimeError::Invalid(_))
        ));
        assert!(lifecycle::running("refused").is_some());
        running.gateway().close();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_gateway_ca_is_added_by_a_root_update_of_the_trust_store() {
        let exec = gateway_ca_command("dev");
        assert_eq!(exec.user.as_deref(), Some("root"));
        assert_eq!(exec.argv, ["update-ca-certificates", "--fresh"]);
        assert!(exec.timeout.is_some());
    }

    #[test]
    fn commands_route_by_the_computer_they_name() {
        let home = tempfile::tempdir().unwrap();
        let paths = paths(home.path());
        assert_eq!(
            route(&paths, &strings(&["list", "--format", "json"])),
            Route::Runtime
        );
        make(home.path(), "box", false);
        assert_eq!(
            route(&paths, &strings(&["inspect", "box", "--format", "json"])),
            Route::Container
        );
        assert_eq!(
            route(&paths, &strings(&["inspect", "vm", "--format", "json"])),
            Route::Runtime
        );
        assert_eq!(
            route(&paths, &strings(&["remove", "--quiet", "box"])),
            Route::Container
        );
        assert_eq!(
            route(&paths, &strings(&["list", "--format", "json"])),
            Route::MergeList
        );
        assert_eq!(
            route(&paths, &strings(&["--silo-github-protocol"])),
            Route::Runtime
        );
        assert_eq!(
            route(
                &paths,
                &strings(&["create", "img", "--name", "new", "--label", RUNTIME_LABEL])
            ),
            Route::Container
        );
        assert_eq!(
            route(&paths, &strings(&["create", "img", "--name", "new"])),
            Route::Runtime
        );
    }

    #[test]
    fn inspect_describes_the_saved_configuration_the_engine_compares_with() {
        let home = tempfile::tempdir().unwrap();
        make(home.path(), "box", false);
        let record = Layout::new(home.path())
            .computer("box")
            .read_record()
            .unwrap();
        let value = inspect_value("box", &record, "Created", &[]);
        assert_eq!(value["status"], "Created");
        assert_eq!(value["config"]["name"], "box");
        assert_eq!(value["config"]["labels"]["silo.managed"], "true");
        assert_eq!(value["config"]["resources"]["memory_mib"], 2048);
        assert_eq!(
            value["config"]["image"]["Oci"]["root_disk"]["size_mib"],
            4096
        );
        assert_eq!(
            value["config"]["mounts"][0]["storage"]["capacity_mib"],
            1024
        );
        assert!(value.get("active_config").is_none());
        let running = inspect_value("box", &record, "Running", &[]);
        assert_eq!(running["active_config"], running["config"]);
        assert!(running["runtime_instance_id"]
            .as_str()
            .unwrap()
            .starts_with("container-box-"));
    }

    #[test]
    fn the_list_adds_container_computers_and_honours_the_label_filter() {
        let home = tempfile::tempdir().unwrap();
        let paths = paths(home.path());
        make(home.path(), "box", true);
        let answer = CommandOutput {
            stdout: r#"[{"name":"vm","status":"Running"}]"#.into(),
            stderr: String::new(),
        };
        let merged = merge_list(
            &paths,
            &strings(&["list", "--label", "silo.managed=true", "--format", "json"]),
            answer,
        )
        .unwrap();
        let rows: Vec<Value> = serde_json::from_str(&merged.stdout).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1]["name"], "box");
        assert_eq!(rows[1]["status"], "Stopped");
        let answer = CommandOutput {
            stdout: "[]".into(),
            stderr: String::new(),
        };
        let none = merge_list(
            &paths,
            &strings(&["list", "--label", "silo.other=1"]),
            answer,
        )
        .unwrap();
        assert_eq!(none.stdout, "[]");
    }

    #[test]
    fn checkpoint_commands_are_routed_by_the_computer_or_the_group_they_name() {
        let home = tempfile::tempdir().unwrap();
        let paths = paths(home.path());
        make(home.path(), "box", false);
        let route = |parts: &[&str]| route(&paths, &strings(parts));
        // A capture goes where its computer lives.
        assert_eq!(
            route(&[
                "snapshot",
                "create",
                "c1",
                "--from-sandbox",
                "box",
                "--group",
                "box"
            ]),
            Route::Container
        );
        assert_eq!(
            route(&[
                "snapshot",
                "create",
                "c1",
                "--from-sandbox",
                "vm",
                "--group",
                "vm"
            ]),
            Route::Runtime
        );
        // Everything else by the group, which only exists once a container checkpoint does.
        assert_eq!(route(&["snapshot", "verify", "box:c1"]), Route::Runtime);
        assert_eq!(
            route(&["restore", "box:c1", "--name", "kid"]),
            Route::Runtime
        );
        let layout = Layout::new(home.path());
        layers::Store::new(&layout)
            .write(&layers::Snapshot::new("box", "c1", Vec::new(), {
                layout.computer("box").read_record().unwrap()
            }))
            .unwrap();
        assert_eq!(route(&["snapshot", "verify", "box:c1"]), Route::Container);
        assert_eq!(
            route(&["snapshot", "remove", "--force", "box:c1", "--quiet"]),
            Route::Container
        );
        assert_eq!(route(&["snapshot", "remove", "vm:c2"]), Route::Runtime);
        assert_eq!(
            route(&["restore", "box:c1", "--name", "kid"]),
            Route::Container
        );
        assert_eq!(
            route(&["restore", "vm:c2", "--name", "kid"]),
            Route::Runtime
        );
        assert_eq!(
            route(&["snapshot", "list", "--group", "box", "--format", "json"]),
            Route::Container
        );
        assert_eq!(
            route(&["snapshot", "list", "--group", "vm", "--format", "json"]),
            Route::Runtime
        );
        assert_eq!(
            route(&["snapshot", "list", "--format", "json"]),
            Route::MergeList
        );
        assert_eq!(route(&["stop", "--force", "box"]), Route::Container);
        assert_eq!(
            route(&["pause", "box", "--guest-flush", "required"]),
            Route::Container
        );
        assert_eq!(route(&["resume", "box"]), Route::Container);
    }

    #[test]
    fn an_imported_containers_checkpoint_is_listed_on_a_device_with_no_container_computer() {
        let home = tempfile::tempdir().unwrap();
        let paths = paths(home.path());
        let list = strings(&["snapshot", "list", "--format", "json"]);
        assert_eq!(route(&paths, &list), Route::Runtime);
        let layout = Layout::new(home.path());
        let template = {
            make(home.path(), "box", false);
            layout.computer("box").read_record().unwrap()
        };
        std::fs::remove_dir_all(layout.computer("box").path()).unwrap();
        assert!(layout.names().is_empty());
        layers::Store::new(&layout)
            .write(&layers::Snapshot::new(
                "silo-import-ab",
                "c1",
                Vec::new(),
                template,
            ))
            .unwrap();
        assert_eq!(route(&paths, &list), Route::MergeList);
        assert_eq!(
            route(
                &paths,
                &strings(&["restore", "silo-import-ab:c1", "--name", "kid"])
            ),
            Route::Container
        );
    }

    #[test]
    fn the_container_quiet_step_tolerates_only_the_host_name_and_hosts_file() {
        let script = include_str!("../runtime/checkpoints/quiet.sh");
        let adapted = quiet_script(script);
        assert_eq!(
            script.matches("|| exit 1").count() - 2,
            adapted.matches("|| exit 1").count()
        );
        assert!(adapted.contains("hostname \"$host\" || true"));
        assert!(adapted.contains("> /etc/hosts || true"));
        // The machine identity and the scheduled-job steps still stop the script when they fail.
        assert!(adapted.contains("printf '%s\\n' \"$new\" > /etc/machine-id || exit 1"));
        assert!(adapted.contains("park \"$f\" || exit 1"));
    }

    #[test]
    fn commands_only_virtual_machines_answer_are_refused_in_plain_words() {
        let home = tempfile::tempdir().unwrap();
        let paths = paths(home.path());
        make(home.path(), "box", false);
        let error = run(
            &paths,
            &strings(&["rename", "box", "x"]),
            Duration::from_secs(1),
            &Material::NONE,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("cannot rename a container computer yet"),
            "{error}"
        );
    }

    fn modify_args(parts: &[&str]) -> Vec<String> {
        let mut args = strings(&["modify", "box"]);
        args.extend(strings(parts));
        args.extend(strings(&["--format", "json"]));
        args
    }

    fn secrets_of(paths: &RuntimePaths) -> Vec<Value> {
        let value = inspect(paths, "box").unwrap();
        value["config"]["network"]["secrets"]["secrets"]
            .as_array()
            .unwrap()
            .clone()
    }

    #[test]
    fn secrets_are_assigned_and_removed_by_the_commands_the_shared_code_issues() {
        let home = tempfile::tempdir().unwrap();
        let paths = paths(home.path());
        make(home.path(), "box", false);
        assert!(secrets_of(&paths).is_empty());
        let assign = modify_args(&[
            "--secret",
            "TOKEN:passthrough=*@api.example.test,*.cdn.example.test",
        ]);
        run(&paths, &assign, Duration::from_secs(1), &Material::NONE).unwrap();
        let secrets = secrets_of(&paths);
        assert_eq!(secrets.len(), 1);
        assert_eq!(secrets[0]["env_var"], "TOKEN");
        assert_eq!(secrets[0]["placeholder"], "$MSB_TOKEN");
        assert_eq!(
            secrets[0]["allowed_hosts"][1]["wildcard"],
            "*.cdn.example.test"
        );
        // Never a value: the record holds names and hosts only.
        let record =
            std::fs::read_to_string(Layout::new(home.path()).computer("box").record_path())
                .unwrap();
        assert!(!record.contains("value"), "{record}");
        let remove = modify_args(&["--secret-rm", "TOKEN"]);
        run(&paths, &remove, Duration::from_secs(1), &Material::NONE).unwrap();
        assert!(secrets_of(&paths).is_empty());
    }

    #[test]
    fn github_access_is_assigned_to_a_stopped_container_and_kept() {
        let home = tempfile::tempdir().unwrap();
        let paths = paths(home.path());
        make(home.path(), "box", false);
        let assign = modify_args(&[
            "--secret",
            "SILO_GITHUB:passthrough=*@github.com,api.github.com,uploads.github.com",
        ]);
        run(&paths, &assign, Duration::from_secs(1), &Material::NONE).unwrap();
        let secrets = secrets_of(&paths);
        assert_eq!(secrets[0]["env_var"], "SILO_GITHUB");
        assert_eq!(secrets[0]["placeholder"], "$MSB_SILO_GITHUB");
        let elsewhere = modify_args(&["--secret", "SILO_GITHUB:passthrough=*@evil.example"]);
        let error = run(&paths, &elsewhere, Duration::from_secs(1), &Material::NONE).unwrap_err();
        assert!(error.to_string().contains("GitHub hosts only"), "{error}");
    }

    #[test]
    fn a_modify_that_changes_anything_but_secrets_is_refused_in_plain_words() {
        let home = tempfile::tempdir().unwrap();
        let paths = paths(home.path());
        make(home.path(), "box", false);
        let error = run(
            &paths,
            &modify_args(&["--cpus", "2", "--next-start"]),
            Duration::from_secs(1),
            &Material::NONE,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("cannot modify a container computer yet"),
            "{error}"
        );
    }

    #[test]
    fn inspecting_a_missing_container_reads_as_not_found() {
        let home = tempfile::tempdir().unwrap();
        let error = inspect(&paths(home.path()), "ghost").unwrap_err();
        assert!(format!("{error:?}").contains("not found"));
    }

    #[test]
    fn creating_over_an_existing_container_is_refused_before_anything_changes() {
        let home = tempfile::tempdir().unwrap();
        let paths = paths(home.path());
        make(home.path(), "box", false);
        let create = Create {
            name: "box".into(),
            cpus: 1,
            max_cpus: 1,
            memory_mib: 1,
            max_memory_mib: 1,
            root_disk_mib: 1,
            workspace_mib: 1,
            labels: vec![],
            env: vec![],
            github: false,
        };
        // On a host that is not ready the refusal names the missing pieces first; either way no record changes.
        let error = create_computer(&paths, &create).unwrap_err();
        assert!(matches!(
            error,
            RuntimeError::Invalid(_) | RuntimeError::Unavailable(_)
        ));
        assert!(Layout::new(home.path()).computer("box").exists());
    }
}
