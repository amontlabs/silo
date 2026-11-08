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

pub mod args;
pub mod capability;
pub mod hold;
pub mod image;
pub mod layout;
#[cfg(target_os = "linux")]
pub mod lifecycle;
pub mod oci;
#[cfg(target_os = "linux")]
pub mod ports;
pub mod secrets;
pub mod tools;
pub mod users;

pub use args::RUNTIME_LABEL;

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

/// Once, when the engine starts: stops the container computers an earlier engine process left
/// running (their gateways went with it). Reading a computer's state never does this.
pub fn reconcile_at_start(paths: &RuntimePaths) {
    #[cfg(target_os = "linux")]
    if !layout::Layout::new(&paths.home).names().is_empty() {
        if let Ok(tools) = tools::Tools::locate(paths) {
            lifecycle::reconcile(paths, &tools);
        }
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
    let mut after = before.clone();
    secrets::apply_to_record(&mut after, modify).map_err(RuntimeError::Invalid)?;
    #[cfg(target_os = "linux")]
    live_update(paths, &before, modify, material)?;
    #[cfg(not(target_os = "linux"))]
    let _ = material;
    dir.update_record(|record| {
        record.secrets = after.secrets.clone();
        record.github = after.github;
    })
    .map(|_| ())
    .map_err(RuntimeError::Unavailable)
}

#[cfg(target_os = "linux")]
fn live_update(
    paths: &RuntimePaths,
    before: &ComputerRecord,
    modify: &args::Modify,
    material: &Material<'_>,
) -> Result<(), RuntimeError> {
    use crate::computer_gateway::Secret;
    use zeroize::Zeroizing;
    let name = modify.name.as_str();
    if lifecycle::status(paths, name, before) != lifecycle::Status::Running {
        return Ok(());
    }
    let Some(running) = lifecycle::running(name) else {
        return Ok(());
    };
    let rejected = |detail: String| RuntimeError::Failed {
        operation: "Updating the computer".into(),
        exit_code: Some(1),
        detail,
    };
    let mut active = running.active_secrets();
    let mut gateway = running.gateway();
    for spec in modify.put.iter().filter(|_| !modify.next_start) {
        if spec.name == args::GITHUB_SECRET_NAME {
            if !before.github {
                return Err(RuntimeError::Invalid(
                    "This computer was started without GitHub access. Restart it to turn GitHub access on."
                        .into(),
                ));
            }
            gateway
                .set_github_profile(Some(material.github_profile))
                .map_err(rejected)?;
            continue;
        }
        let Some((_, value, hosts)) = material
            .secrets
            .iter()
            .find(|(secret, _, _)| secret == &spec.name)
        else {
            return Err(RuntimeError::Invalid(format!(
                "The value of secret {} is not available to the engine.",
                spec.name
            )));
        };
        gateway
            .put_secret(&Secret {
                name: spec.name.clone(),
                value: Zeroizing::new(value.clone()),
                allowed_hosts: hosts.clone(),
            })
            .map_err(rejected)?;
        if !active.contains(&spec.name) {
            active.push(spec.name.clone());
        }
        running.set_active(active.clone());
    }
    for removed in &modify.remove {
        // The gateway refuses to remove a secret it does not hold, which would stop it.
        if active.contains(removed) {
            gateway.remove_secret(removed).map_err(rejected)?;
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
        created_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
        started_once: false,
        secrets: Vec::new(),
        github: create.github,
    }
}

/// The state a read reports. It only looks: a list or an inspect never changes a computer.
#[cfg(target_os = "linux")]
fn state_word(paths: &RuntimePaths, name: &str, record: &ComputerRecord) -> &'static str {
    match lifecycle::status(paths, name, record) {
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
        "labels": labels,
        "resources": {
            "cpus": record.cpus,
            "max_cpus": record.max_cpus,
            "memory_mib": record.memory_mib,
            "max_memory_mib": record.max_memory_mib,
        },
        "image": {"Oci": {"reference": record.image_reference, "root_disk": {"size_mib": record.root_disk_mib}}},
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
    if status == "Running" {
        value["active_config"] = value["config"].clone();
        value["active_config"]["network"] = secrets::network_value(record, Some(active));
        value["runtime_instance_id"] = json!(format!("container-{name}-{}", record.created_ms));
    }
    value
}

fn inspect(paths: &RuntimePaths, name: &str) -> Result<Value, RuntimeError> {
    let record = record_of(paths, name)?;
    Ok(inspect_value(
        name,
        &record,
        state_word(paths, name, &record),
        &active_secrets(name),
    ))
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

/// MicroSandbox's `list` answer with the container computers added.
pub fn merge_list(
    paths: &RuntimePaths,
    args: &[String],
    answer: CommandOutput,
) -> Result<CommandOutput, RuntimeError> {
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
    lifecycle::start(paths, &tools, name, timeout, material)
}

#[cfg(target_os = "linux")]
fn stop(paths: &RuntimePaths, name: &str) -> Result<(), RuntimeError> {
    let record = record_of(paths, name)?;
    let tools = tools::Tools::locate(paths)?;
    lifecycle::stop(paths, &tools, name, &record)
}

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
    fn commands_only_virtual_machines_answer_are_refused_in_plain_words() {
        let home = tempfile::tempdir().unwrap();
        let paths = paths(home.path());
        make(home.path(), "box", false);
        let error = run(
            &paths,
            &strings(&["snapshot", "create", "--sandbox", "box", "x"]),
            Duration::from_secs(1),
            &Material::NONE,
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("cannot snapshot a container computer yet"),
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
        assert_eq!(secrets[0]["allowed_hosts"][1]["wildcard"], "*.cdn.example.test");
        // Never a value: the record holds names and hosts only.
        let record = std::fs::read_to_string(
            Layout::new(home.path()).computer("box").record_path(),
        )
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
            error.to_string().contains("cannot modify a container computer yet"),
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
