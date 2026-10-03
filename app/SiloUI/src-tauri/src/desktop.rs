//! An optional guest desktop. Agent tools are independent consumers of its X session.
use crate::runtime::{self, ComputerConfiguration, RuntimeError, RuntimePaths, RuntimeRunner};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopConfiguration {
    #[serde(default = "default_start")]
    pub start_with_computer: bool,
    /// The desktop and computer use come with the guest image (v4 and later) and always
    /// start with the computer. Silo decides this when it creates the computer; a value sent with a
    /// saved configuration is ignored (see `keep_built_in`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub built_in: bool,
}
fn default_start() -> bool {
    true
}

pub(crate) fn configuration(
    configuration: &ComputerConfiguration,
) -> Option<&DesktopConfiguration> {
    configuration.desktop.as_ref()
}
/// Guest images from v4 on contain the desktop. Their version looks like `ubuntu-24.04-v4`.
pub(crate) fn image_includes_desktop(image_version: &str) -> bool {
    image_version
        .rsplit_once("-v")
        .and_then(|(_, revision)| revision.parse::<u32>().ok())
        .is_some_and(|revision| revision >= 4)
}

/// On such an image the desktop is part of every new computer, started with it (computer use
/// needs a running session). Existing computers and explicit choices are left alone, except
/// that `built_in` is Silo's to decide: an existing computer keeps what it had and a new computer
/// has it exactly when it is created from such an image, whatever a request says; a new
/// built-in computer also always starts its desktop with the computer.
pub(crate) fn default_new_computer_desktops(
    computers: &mut [ComputerConfiguration],
    previous: &[ComputerConfiguration],
    image_version: &str,
) {
    let built_in_image = image_includes_desktop(image_version);
    for configuration in computers {
        let old = previous.iter().find(|old| old.id() == configuration.id());
        let desktop = &mut configuration.desktop;
        match old {
            Some(old) => {
                let was_built_in =
                    crate::desktop::configuration(old).is_some_and(|old| old.built_in);
                if let Some(configuration) = desktop {
                    configuration.built_in = was_built_in;
                }
            }
            None if built_in_image => {
                // Computer use needs the session running, so a new built-in computer always
                // starts it, including settings duplicated from a legacy computer that chose
                // to start its desktop by hand.
                let configuration = desktop.get_or_insert(DesktopConfiguration {
                    start_with_computer: true,
                    built_in: true,
                });
                configuration.built_in = true;
                configuration.start_with_computer = true;
            }
            None => {
                if let Some(configuration) = desktop {
                    configuration.built_in = false;
                }
            }
        }
    }
}

pub(crate) fn only_desktop_changed(
    previous: &ComputerConfiguration,
    next: &ComputerConfiguration,
) -> bool {
    let mut previous = previous.clone();
    let mut next = next.clone();
    let changed = previous.desktop != next.desktop;
    previous.desktop = None;
    next.desktop = None;
    changed && previous == next
}

pub(crate) fn guest(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    name: &str,
    script: &str,
    timeout: Duration,
    allow_boot: bool,
) -> Result<String, RuntimeError> {
    guest_within(
        runner,
        paths,
        name,
        script,
        timeout,
        Duration::from_secs(60),
        allow_boot,
    )
}

/// `guest` with an explicit allowance on top of the guest-side `timeout` for the host's
/// own wait (starting the command and collecting its output).
pub(crate) fn guest_within(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    name: &str,
    script: &str,
    timeout: Duration,
    grace: Duration,
    allow_boot: bool,
) -> Result<String, RuntimeError> {
    let mut args = vec![
        "exec".into(),
        name.into(),
        "--no-tty".into(),
        "--quiet".into(),
        "--timeout".into(),
        format!("{}s", timeout.as_secs()),
        "--user".into(),
        "root".into(),
        "--workdir".into(),
        "/".into(),
        "--".into(),
        "sh".into(),
        "-c".into(),
        script.into(),
    ];
    if !allow_boot {
        args.insert(2, "--no-start".into());
    }
    let output = runner.run(paths, &args, timeout + grace)?;
    Ok(output.stdout)
}

/// The pinned streamer recipe this Silo installs.
const STREAMER_LOCK: &str = include_str!("../guest/desktop-streamer-lock.json");
/// The oldest installed streamer recipe revision the bundled guest helper still runs.
const OLDEST_RUNNABLE_RECIPE: u64 = 1;
const STREAMER_RECEIPT: &str = "/var/lib/silo-desktop/streamer.json";

fn bundled_recipe_version() -> Option<u64> {
    serde_json::from_str::<Value>(STREAMER_LOCK)
        .ok()?
        .get("recipeVersion")?
        .as_u64()
}

/// Derives update availability from the installed receipt rather than from the installed
/// helper, whose own threshold may predate the bundled recipe. A runnable older recipe is
/// an optional update; the helper's requirement only stands for recipes that cannot run.
fn apply_receipt_recipe(status: &mut Value, receipt: Option<&Value>, bundled: Option<u64>) {
    let (Some(bundled), Some(receipt)) = (bundled, receipt) else {
        return;
    };
    if receipt.get("backend").and_then(Value::as_str) != Some("selkies") {
        return;
    }
    let Some(installed) = receipt.get("recipeVersion").and_then(Value::as_u64) else {
        return;
    };
    if (OLDEST_RUNNABLE_RECIPE..bundled).contains(&installed) {
        status["updateRequired"] = json!(false);
        status["updateAvailable"] = json!(true);
    } else if installed == bundled {
        status["updateRequired"] = json!(false);
        status["updateAvailable"] = json!(false);
    }
}

// Stage the bundled sources for first installation and explicit repairs/updates.
// An existing guest helper may predate these pinned inputs, so never delegate
// installation or repair to an older copy.
fn installer_script(action: &str) -> String {
    let mut script = String::from("set -eu\ndesktop_stage=$(mktemp -d /tmp/silo-desktop.XXXXXXXX)\ntrap 'rm -rf \"$desktop_stage\"' EXIT\n");
    for (variable, filename, source, delimiter) in [
        (
            "SILO_DESKTOP_SERVICE_SOURCE",
            "desktop-service.py",
            include_str!("../guest/desktop-service.py"),
            "SILO_DESKTOP_SERVICE_EOF",
        ),
        (
            "SILO_DESKTOP_STREAMER_LOCK_SOURCE",
            "desktop-streamer-lock.json",
            STREAMER_LOCK,
            "SILO_DESKTOP_STREAMER_LOCK_EOF",
        ),
        (
            "SILO_SELKIES_WEB_CLIENT_PATCH_SOURCE",
            "patch-selkies-web-client.py",
            include_str!("../guest/patch-selkies-web-client.py"),
            "SILO_SELKIES_WEB_CLIENT_PATCH_EOF",
        ),
        (
            "SILO_DESKTOP_PACKAGES_SOURCE",
            "desktop-packages.txt",
            include_str!("../guest/desktop-packages.txt"),
            "SILO_DESKTOP_PACKAGES_EOF",
        ),
        (
            "SILO_ACCESSIBILITY_HELPER_SOURCE",
            "silo-accessibility.py",
            include_str!("../guest/silo-accessibility.py"),
            "SILO_ACCESSIBILITY_HELPER_EOF",
        ),
    ] {
        script.push_str(&format!("export {variable}=\"$desktop_stage/{filename}\"\ncat > \"${variable}\" <<'{delimiter}'\n{source}\n{delimiter}\n"));
    }
    script.push_str(&format!("set -- {action}\n(\n"));
    script.push_str(include_str!("../guest/setup-desktop.sh"));
    script.push_str("\n)\n");
    script
}

fn lcu_setup_script() -> String {
    let mut script = String::from(
        "set -eu\nlcu_stage=$(mktemp -d /tmp/silo-lcu.XXXXXXXX)\ntrap 'rm -rf \"$lcu_stage\"' EXIT\n",
    );
    for (filename, source, delimiter) in [
        (
            "setup-lcu.py",
            include_str!("../guest/setup-lcu.py"),
            "SILO_LCU_SETUP_EOF",
        ),
        (
            "lcu-lock.json",
            include_str!("../guest/lcu-legacy-lock.json"),
            "SILO_LCU_LOCK_EOF",
        ),
    ] {
        script.push_str(&format!(
            "cat > \"$lcu_stage/{filename}\" <<'{delimiter}'\n{source}\n{delimiter}\n"
        ));
    }
    script.push_str(
        r#"lcu_status=$(python3 "$lcu_stage/setup-lcu.py" status)
if printf '%s\n' "$lcu_status" | python3 -c 'import json,sys; raise SystemExit(0 if json.load(sys.stdin).get("status") == "needs-runtime" else 1)'; then printf '%s\n' "$lcu_status"; exit 0; fi
install -d -m 0755 /usr/local/libexec /usr/local/share/silo
install -m 0755 "$lcu_stage/setup-lcu.py" /usr/local/libexec/silo-setup-lcu.py
install -m 0644 "$lcu_stage/lcu-lock.json" /usr/local/share/silo/lcu-lock.json
python3 /usr/local/libexec/silo-setup-lcu.py setup
"#,
    );
    script
}

fn action_script(action: &str) -> String {
    if action == "setup-lcu" {
        lcu_setup_script()
    } else if action == "update-streamer" {
        installer_script(action)
    } else {
        format!("/usr/local/bin/silo-desktop {action}")
    }
}

fn action_timeout(action: &str) -> Duration {
    if matches!(
        action,
        "update-streamer" | "setup-lcu" | "setup-computer-use"
    ) {
        Duration::from_secs(1800)
    } else if action == "restart-streamer" {
        Duration::from_secs(120)
    } else {
        Duration::from_secs(60)
    }
}

/// How long the operation queue should treat a desktop action as healthy: the
/// guest command's own limit plus time to start the computer (G-14).
fn action_expected_duration(action: &str) -> Duration {
    (action_timeout(action) + Duration::from_secs(5 * 60)).max(Duration::from_secs(10 * 60))
}

fn action_starts_computer(action: &str) -> bool {
    action == "start"
}

pub(crate) fn configure_with(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    name: &str,
    previous: Option<&DesktopConfiguration>,
    desired: &DesktopConfiguration,
) -> Result<(), RuntimeError> {
    let inspected = runtime::inspect_computer(runner, paths, name)?;
    runtime::ensure_managed(&inspected)?;
    let mut script = String::new();
    if previous.is_none() {
        let capability = runner.run(
            paths,
            &["--silo-desktop-protocol".into()],
            Duration::from_secs(10),
        )?;
        if capability.stdout.trim() != "1" {
            return Err(RuntimeError::Unavailable("The bundled runtime does not support desktop startup. Repair or update Silo before adding a desktop.".into()));
        }
        script.push_str(&installer_script("install"));
    }
    script.push_str(&format!(
        "/usr/local/bin/silo-desktop autostart {}\n",
        desired.start_with_computer
    ));
    guest(
        runner,
        paths,
        name,
        &script,
        Duration::from_secs(1800),
        true,
    )?;
    Ok(())
}

fn computer_configuration(
    app: &AppHandle,
    computer: &str,
    expected_id: Option<&str>,
) -> Result<(RuntimePaths, ComputerConfiguration), String> {
    runtime::validate_name(computer).map_err(|e| e.to_string())?;
    let paths = runtime::runtime_paths(app)?;
    let configuration = computer_at(&runtime::ProcessRunner, &paths, computer, expected_id)?;
    Ok((paths, configuration))
}

fn computer_at(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer: &str,
    expected_id: Option<&str>,
) -> Result<ComputerConfiguration, String> {
    let configuration = runtime::read_metadata(&paths.metadata)
        .map_err(|e| e.to_string())?
        .computers
        .into_iter()
        .find(|m| m.name() == computer)
        .ok_or("This computer no longer exists on this device.")?;
    if expected_id.is_some_and(|id| configuration.id() != id) {
        return Err("The computer changed identity. Refresh before accessing its desktop.".into());
    }
    // An imported computer waiting for its first Start has no runtime computer yet: Silo's own
    // record is all there is, and the status and approval paths treat it as stopped.
    if runtime::is_pending_restore(paths, computer) {
        return Ok(configuration);
    }
    let inspected =
        runtime::inspect_computer(runner, paths, computer).map_err(|e| e.to_string())?;
    ensure_computer_identity(&configuration, &inspected)?;
    Ok(configuration)
}

fn ensure_computer_identity(
    configuration: &ComputerConfiguration,
    inspected: &runtime::InspectedSandbox,
) -> Result<(), String> {
    runtime::ensure_managed(inspected).map_err(|e| e.to_string())?;
    if inspected.name != configuration.name()
        || inspected
            .config
            .pointer("/labels/silo.machine-id")
            .and_then(Value::as_str)
            != Some(configuration.id())
    {
        return Err("The computer changed identity. Refresh before accessing its desktop.".into());
    }
    Ok(())
}

/// The desktop state a live regression polls (production code path, real runtime).
#[cfg(test)]
pub(crate) fn test_status(
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
) -> Result<Value, String> {
    status_with(&runtime::ProcessRunner, paths, configuration)
}

fn status_with(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
) -> Result<Value, String> {
    let settings = crate::desktop::configuration(configuration);
    let built_in = crate::computer_use::is_built_in(configuration);
    let fallback = |state: &str| {
        let mut value = json!({"installed": settings.is_some(), "state":state, "autoStart":settings.is_some_and(|s| s.start_with_computer), "backend":null, "sessionState":"stopped", "streamState":"stopped", "updateRequired":false, "updateAvailable":false, "streamerVersion":null, "lcuState":null, "lcuReason":null, "lcuVersion":null, "lcuAppVersion":null, "lcuRuntimeVersion":null, "lcuAgents":null, "lcuReadiness":null});
        // A stopped computer still reports its approval mode and the last versions it had.
        if let Some(computer_use) =
            crate::computer_use::desktop_state(paths, configuration, false, None)
        {
            value["computerUse"] = computer_use;
        }
        value
    };
    let inspected = match runtime::observe_computer(runner, paths, configuration.name())
        .map_err(|e| e.to_string())?
    {
        runtime::ComputerRuntime::Present(inspected) => {
            ensure_computer_identity(configuration, &inspected)?;
            Some(inspected)
        }
        runtime::ComputerRuntime::Absent => None,
    };
    if inspected
        .as_ref()
        .is_none_or(|inspected| inspected.status != "Running")
    {
        return Ok(fallback(if settings.is_some() {
            "computer-stopped"
        } else {
            "uninstalled"
        }));
    }
    let mut script = String::from("if [ -x /usr/local/bin/silo-desktop ]; then /usr/local/bin/silo-desktop status; else printf '%s\\n' '{\"installed\":false,\"state\":\"uninstalled\",\"autoStart\":false}'; fi");
    if built_in {
        script.push('\n');
        script.push_str(crate::computer_use::STATUS_COMMAND);
    }
    script.push_str(&format!(
        "\nprintf '%s\\n' \"$(tr -d '\\n\\r' < {STREAMER_RECEIPT} 2>/dev/null || true)\"\n"
    ));
    let output = guest(
        runner,
        paths,
        configuration.name(),
        &script,
        Duration::from_secs(15),
        false,
    )
    .map_err(|e| e.to_string())?;
    let mut lines = output.trim().lines();
    let value: Value = serde_json::from_str(lines.next().unwrap_or("").trim())
        .map_err(|_| "The desktop returned an invalid status.")?;
    let mut status = public_status(value)?;
    let guest_state = built_in
        .then(|| lines.next())
        .flatten()
        .and_then(|line| serde_json::from_str::<Value>(line.trim()).ok());
    let receipt = lines
        .next()
        .and_then(|line| serde_json::from_str::<Value>(line.trim()).ok());
    apply_receipt_recipe(&mut status, receipt.as_ref(), bundled_recipe_version());
    if built_in {
        if let Some(computer_use) =
            crate::computer_use::desktop_state(paths, configuration, true, guest_state.as_ref())
        {
            status["computerUse"] = computer_use;
        }
    }
    Ok(status)
}

// Project explicit public fields: credentials and arbitrary guest output never reach the UI.
fn public_status(value: Value) -> Result<Value, String> {
    let state = value["state"]
        .as_str()
        .filter(|s| {
            matches!(
                *s,
                "running" | "stopped" | "failed" | "uninstalled" | "starting"
            )
        })
        .ok_or("The desktop returned an unknown session state.")?;
    let installed = value["installed"]
        .as_bool()
        .ok_or("The desktop returned an invalid installation state.")?;
    let auto_start = value["autoStart"]
        .as_bool()
        .ok_or("The desktop returned an invalid startup preference.")?;
    let legacy_component_state = || {
        value["state"]
            .as_str()
            .filter(|state| matches!(*state, "running" | "starting" | "stopped" | "failed"))
    };
    let backend = match value.get("backend") {
        None if installed => Some("kasm"),
        None => None,
        Some(Value::Null) => None,
        Some(Value::String(backend)) if matches!(backend.as_str(), "kasm" | "selkies") => {
            Some(backend.as_str())
        }
        Some(_) => return Err("The desktop returned an invalid streamer backend.".into()),
    };
    let component_state = |field: &str| -> Result<Option<&str>, String> {
        match value.get(field) {
            None => Ok(legacy_component_state()),
            Some(Value::Null) => Ok(None),
            Some(Value::String(state))
                if matches!(
                    state.as_str(),
                    "stopped" | "starting" | "running" | "failed"
                ) =>
            {
                Ok(Some(state.as_str()))
            }
            Some(_) => Err("The desktop returned an invalid component state.".into()),
        }
    };
    let session_state = component_state("sessionState")?;
    let stream_state = component_state("streamState")?;
    let update_required = match value.get("updateRequired") {
        None => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => return Err("The desktop returned an invalid update requirement.".into()),
    };
    let update_available = match value.get("updateAvailable") {
        None => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => return Err("The desktop returned an invalid update availability.".into()),
    };
    let streamer_version = value["streamerVersion"].as_str().filter(|version| {
        let parts = version.split('.').collect::<Vec<_>>();
        parts.len() == 3
            && parts
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    });
    let lcu_state = value["lcuState"].as_str().filter(|state| {
        matches!(
            *state,
            "needs-runtime"
                | "not-installed"
                | "repair-required"
                | "failed"
                | "installing"
                | "ready"
        )
    });
    let lcu_reason = value["lcuReason"].as_str().filter(|reason| {
        matches!(
            *reason,
            "chatgpt-app-required"
                | "invalid-receipt"
                | "unsupported-receipt"
                | "managed-runtime-missing"
                | "setup-failed"
        )
    });
    let lcu_version = safe_version(value["lcuVersion"].as_str());
    let lcu_app_version = safe_version(value["lcuAppVersion"].as_str());
    let lcu_runtime_version = safe_lcu_runtime_version(value["lcuRuntimeVersion"].as_str());
    let lcu_agents = value["lcuAgents"].as_array().map(|agents| {
        agents
            .iter()
            .filter_map(Value::as_str)
            .filter(|agent| matches!(*agent, "pi" | "codex" | "claude-code"))
            .collect::<Vec<_>>()
    });
    let lcu_readiness = value["lcuReadiness"]
        .as_str()
        .filter(|readiness| matches!(*readiness, "ready" | "unverified" | "failed"));
    Ok(
        json!({"installed":installed,"state":state,"autoStart":auto_start,
        "version":safe_version(value["version"].as_str()),"user":safe_user(value["user"].as_str()),
        "display":safe_display(value["display"].as_str()),
        "lcuState":lcu_state,
        "lcuReason":lcu_reason, "lcuVersion":lcu_version,
        "lcuAppVersion":lcu_app_version, "lcuRuntimeVersion":lcu_runtime_version,
        "lcuAgents":lcu_agents, "lcuReadiness":lcu_readiness, "backend":backend,
        "sessionState":session_state, "streamState":stream_state,
        "updateRequired":update_required, "updateAvailable":update_available && !update_required,
        "streamerVersion":streamer_version}),
    )
}

/// A POSIX account name as the guest reports it; anything else is dropped.
fn safe_user(value: Option<&str>) -> Option<&str> {
    value.filter(|user| {
        let mut bytes = user.bytes();
        bytes
            .next()
            .is_some_and(|first| first.is_ascii_lowercase() || first == b'_')
            && user.len() <= 32
            && bytes.all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(&byte)
            })
    })
}

/// An X display such as `:1` or `:1.0`.
fn safe_display(value: Option<&str>) -> Option<&str> {
    value.filter(|display| {
        display.len() <= 32
            && display.len() > 1
            && display.starts_with(':')
            && display[1..]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'.')
    })
}

fn safe_version(value: Option<&str>) -> Option<&str> {
    value.filter(|version| {
        !version.is_empty()
            && version.len() <= 64
            && version
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b".+~:-_".contains(&byte))
    })
}

fn safe_lcu_runtime_version(value: Option<&str>) -> Option<&str> {
    value.filter(|runtime| {
        if runtime.len() > 64 {
            return false;
        }
        let Some((version, build)) = runtime.split_once('/') else {
            return false;
        };
        let components = version.split('.').collect::<Vec<_>>();
        let build = build.as_bytes();
        components.len() == 3
            && components.iter().all(|component| {
                !component.is_empty() && component.bytes().all(|byte| byte.is_ascii_digit())
            })
            && build.len() == 27
            && build[..14].iter().all(u8::is_ascii_digit)
            && build[14] == b'-'
            && build[15..]
                .iter()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    })
}

pub(crate) fn dispatch(app: &AppHandle, method: &str, params: &Value) -> Result<Value, String> {
    let computer_id = params["computerId"]
        .as_str()
        .ok_or("Missing computer identity.")?;
    let name = runtime::remote_ops::local_computer_name(app, computer_id)?;
    if method == "computerUse.approval" {
        return local_approval(
            app,
            &name,
            params["mode"].as_str().ok_or("Missing approval mode.")?,
            Some(computer_id),
        );
    }
    local(
        app,
        &name,
        if method == "desktop.status" {
            None
        } else {
            Some(params["action"].as_str().ok_or("Missing desktop action.")?)
        },
        Some(computer_id),
    )
}

fn prepare_local<'a>(
    gate: &'a runtime::operation_gate::OperationGate,
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer: &str,
    action: Option<&str>,
    expected_id: Option<&str>,
) -> Result<
    (
        Option<runtime::operation_gate::OperationGuard<'a>>,
        ComputerConfiguration,
    ),
    String,
> {
    let computer_id = match expected_id {
        Some(id) => Some(id.to_owned()),
        None if action.is_some() => {
            Some(runtime::resolve_computer_id(paths, computer).map_err(|e| e.to_string())?)
        }
        None => None,
    };
    let guard = match action {
        Some(action) => {
            let computer_id = computer_id.as_deref().ok_or("Missing computer identity.")?;
            let guard = gate
                .computer(
                    computer_id,
                    computer,
                    &format!("Updating {computer} desktop"),
                )
                .map_err(|e| e.to_string())?;
            // Desktop/guest setup is cancellable; installs legitimately run up
            // to their guest timeout, so only flag them after that.
            guard.allow_cancel();
            guard.expect_within(action_expected_duration(action));
            Some(guard)
        }
        None => None,
    };
    runtime::validate_name(computer).map_err(|e| e.to_string())?;
    let configuration = computer_at(runner, paths, computer, computer_id.as_deref())?;
    Ok((guard, configuration))
}

fn local(
    app: &AppHandle,
    computer: &str,
    action: Option<&str>,
    expected_id: Option<&str>,
) -> Result<Value, String> {
    // A desktop action changes only this computer's guest (and may start the computer); it
    // waits its turn per computer. Reading desktop status observes only, so it takes
    // no gate and stays available during other operations.
    let paths = runtime::runtime_paths(app)?;
    let (_guard, configuration) = prepare_local(
        &runtime::OPERATIONS,
        &runtime::ProcessRunner,
        &paths,
        computer,
        action,
        expected_id,
    )?;
    if let Some(action) = action {
        runtime::shutdown::ensure_accepting_operations()?;
        if !matches!(
            action,
            "start"
                | "stop"
                | "restart"
                | "restart-streamer"
                | "update-streamer"
                | "setup-lcu"
                | "setup-computer-use"
        ) {
            return Err("Unsupported desktop action.".into());
        }
        if crate::desktop::configuration(&configuration).is_none() {
            return Err("Add a Linux desktop in computer settings first.".into());
        }
        if action == "setup-computer-use" && !crate::computer_use::is_built_in(&configuration) {
            return Err(
                "Computer use is built into computers created with the current guest image. Create a new computer to use it."
                    .into(),
            );
        }
        if action == "setup-lcu" && crate::computer_use::is_built_in(&configuration) {
            return Err("This computer sets up computer use itself.".into());
        }
        let inspected = match runtime::observe_computer(&runtime::ProcessRunner, &paths, computer)
            .map_err(|e| e.to_string())?
        {
            runtime::ComputerRuntime::Absent => return Err(crate::terminal::start_first(computer)),
            runtime::ComputerRuntime::Present(inspected) => {
                ensure_computer_identity(&configuration, &inspected)?;
                inspected
            }
        };
        if action_starts_computer(action)
            && matches!(inspected.status.as_str(), "Created" | "Stopped")
        {
            runtime::start_for_desktop(&paths, computer).map_err(|e| e.to_string())?;
        } else if inspected.status != "Running" {
            return Err("Start the computer before changing its desktop session.".into());
        }
        if action == "setup-computer-use" {
            let token = _guard
                .as_ref()
                .map(runtime::operation_gate::OperationGuard::cancel_token)
                .ok_or("Computer operation ordering failed.")?;
            crate::computer_use::setup_with(
                &runtime::OPERATIONS,
                &runtime::ProcessRunner,
                &paths,
                &configuration,
                true,
                token,
            )
            .map_err(|e| e.to_string())?;
        } else {
            guest(
                &runtime::ProcessRunner,
                &paths,
                computer,
                &action_script(action),
                action_timeout(action),
                false,
            )
            .map_err(|e| e.to_string())?;
        }
        let _ = app.emit("silo://application-state-changed", ());
    }
    status_with(&runtime::ProcessRunner, &paths, &configuration)
}

/// Stores a computer's computer-use approval mode and, when it runs, starts applying it in the
/// background: the answer is the desktop state at once, with the apply `pending`.
fn local_approval(
    app: &AppHandle,
    computer: &str,
    mode: &str,
    expected_id: Option<&str>,
) -> Result<Value, String> {
    let approval =
        crate::computer_use::Approval::parse(mode).ok_or("Unsupported computer-use approval.")?;
    runtime::shutdown::ensure_accepting_operations()?;
    let (paths, configuration) = computer_configuration(app, computer, expected_id)?;
    let status = approval_at(
        &runtime::ProcessRunner,
        &paths,
        &configuration,
        approval,
        std::sync::Arc::new(runtime::ProcessRunner),
    )?;
    let _ = app.emit("silo://application-state-changed", ());
    Ok(status)
}

/// Stores the approval mode and starts applying it to a running computer. A stopped or
/// pending-restore computer keeps it for its next boot, without any guest access.
fn approval_at(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
    approval: crate::computer_use::Approval,
    apply_runner: crate::computer_use::SharedRunner,
) -> Result<Value, String> {
    if !crate::computer_use::is_built_in(configuration) {
        return Err(
            "Computer use is built into computers created with the current guest image.".into(),
        );
    }
    let running = match runtime::observe_computer(runner, paths, configuration.name())
        .map_err(|e| e.to_string())?
    {
        runtime::ComputerRuntime::Present(inspected) => {
            ensure_computer_identity(configuration, &inspected)?;
            inspected.status == "Running"
        }
        runtime::ComputerRuntime::Absent => false,
    };
    crate::computer_use::apply_approval_with(apply_runner, paths, configuration, approval, running)
        .map_err(|e| e.to_string())?;
    status_with(runner, paths, configuration)
}

/// A computer's approval mode for computer use: "ask" (the harness asks first) or "auto".
/// Returns the computer's desktop state. Routed to the device that owns the computer.
#[tauri::command]
pub async fn set_computer_use_approval(
    app: AppHandle,
    window: tauri::Window,
    computer: String,
    mode: String,
) -> Result<Value, String> {
    crate::desktop_viewer::require_computer(&window, &computer)?;
    tauri::async_runtime::spawn_blocking(move || {
        if let Some((device, computer)) = crate::remote_access::target(&computer)? {
            crate::chatgpt_app::call_owner(
                &app,
                &device,
                "computerUse.approval",
                json!({"computerId":computer,"mode":mode}),
            )
        } else {
            local_approval(&app, &computer, &mode, None)
        }
    })
    .await
    .map_err(|_| "Silo could not change computer use. Retry.".to_string())?
}

async fn execute(
    app: AppHandle,
    computer: String,
    action: Option<String>,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if let Some((device, computer)) = crate::remote_access::target(&computer)? {
            let setup = action.as_deref() == Some("setup-computer-use");
            crate::remote::call_remote(
                &app,
                &device,
                if action.is_some() {
                    "desktop.action"
                } else {
                    "desktop.status"
                },
                json!({"computerId":computer,"action":action}),
            )
            // An older Silo there rejects the action it does not know.
            .map_err(|error| {
                if setup && error == "Unsupported desktop action." {
                    "Update Silo on that device to use computer use.".into()
                } else {
                    error
                }
            })
        } else {
            local(&app, &computer, action.as_deref(), None)
        }
    })
    .await
    .map_err(|_| {
        "Silo could not finish the Linux desktop action. Reopen the viewer and retry.".to_string()
    })?
}
#[tauri::command]
pub async fn read_desktop_state(
    app: AppHandle,
    window: tauri::Window,
    computer: String,
) -> Result<Value, String> {
    crate::desktop_viewer::require_computer(&window, &computer)?;
    execute(app, computer, None).await
}
#[tauri::command]
pub async fn desktop_action(
    app: AppHandle,
    window: tauri::Window,
    computer: String,
    action: String,
) -> Result<Value, String> {
    crate::desktop_viewer::require_computer(&window, &computer)?;
    execute(app, computer, Some(action)).await
}

/// Private backend-only connection material. Never register this as a UI command.
pub(crate) fn connection_local(
    app: &AppHandle,
    computer: &str,
    expected_id: Option<&str>,
) -> Result<Value, String> {
    runtime::validate_name(computer).map_err(|e| e.to_string())?;
    let paths = runtime::runtime_paths(app)?;
    connection_with(&runtime::ProcessRunner, &paths, computer, expected_id)
}

fn connection_with(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer: &str,
    expected_id: Option<&str>,
) -> Result<Value, String> {
    runtime::validate_name(computer).map_err(|e| e.to_string())?;
    let configuration = computer_at(runner, paths, computer, expected_id)?;
    if status_with(runner, paths, &configuration)?["state"] != "running" {
        return Err("The desktop is not running.".into());
    }
    let output = guest(
        runner,
        paths,
        computer,
        "/usr/local/bin/silo-desktop connection",
        Duration::from_secs(15),
        false,
    )
    .map_err(|_| "Could not read desktop connection credentials.")?;
    // A read stays ungated; reject credentials if the computer changed while it ran.
    computer_at(runner, paths, computer, Some(configuration.id()))?;
    let value: Value = serde_json::from_str(output.trim())
        .map_err(|_| "Invalid desktop connection credentials.")?;
    let username = value["username"].as_str().filter(|s| {
        !s.is_empty()
            && s.len() <= 64
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    });
    let password = value["password"]
        .as_str()
        .filter(|s| s.len() >= 32 && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_hexdigit()));
    if value["port"].as_u64() != Some(6901) || username.is_none() || password.is_none() {
        return Err("Invalid desktop connection credentials.".into());
    }
    Ok(json!({"port":6901,"username":username,"password":password}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::paths;
    use crate::test_support::runner::{ExpectedCommand, ScriptedRunner};

    fn inspect(status: &str, labels: Value) -> ExpectedCommand {
        ExpectedCommand::ok(
            ["inspect", "dev", "--format", "json"],
            json!({"name":"dev","status":status,"config":{"labels":labels}}).to_string(),
        )
        .with_timeout(Duration::from_secs(10))
    }

    fn computer(id: &str, desktop: Option<DesktopConfiguration>) -> ComputerConfiguration {
        ComputerConfiguration {
            id: id.into(),
            name: format!("computer-{id}"),
            cpus: 2,
            max_cpus: 4,
            memory_gib: 4,
            max_memory_gib: 8,
            workspace_storage_gib: 10,
            runtime_storage_gib: 10,
            desktop,
        }
    }

    #[test]
    fn only_v4_and_later_images_include_the_desktop() {
        for version in ["ubuntu-24.04-v4", "ubuntu-24.04-v5", "ubuntu-24.04-v12"] {
            assert!(image_includes_desktop(version), "{version}");
        }
        for version in [
            "ubuntu-24.04-v3",
            "ubuntu-24.04-v1",
            "ubuntu-24.04",
            "",
            "v4x",
            "ubuntu-24.04-vx",
        ] {
            assert!(!image_includes_desktop(version), "{version}");
        }
    }

    #[test]
    fn new_computers_on_a_v4_image_get_an_autostarting_desktop() {
        let manual = DesktopConfiguration {
            start_with_computer: false,
            built_in: false,
        };
        let existing = computer("existing", None);
        let mut computers = vec![
            existing.clone(),
            computer("fresh", None),
            computer("chosen", Some(manual.clone())),
        ];
        default_new_computer_desktops(
            &mut computers,
            std::slice::from_ref(&existing),
            "ubuntu-24.04-v4",
        );
        assert_eq!(
            configuration(&computers[0]),
            None,
            "existing computers keep their configuration"
        );
        assert_eq!(
            configuration(&computers[1]),
            Some(&DesktopConfiguration {
                start_with_computer: true,
                built_in: true,
            })
        );
        assert_eq!(
            configuration(&computers[2]),
            Some(&DesktopConfiguration {
                start_with_computer: true,
                built_in: true,
            }),
            "a new built-in computer always starts its desktop, even from duplicated manual settings"
        );
    }

    #[test]
    fn existing_legacy_computers_keep_a_manual_desktop_start() {
        let manual = DesktopConfiguration {
            start_with_computer: false,
            built_in: false,
        };
        let previous = vec![computer("legacy", Some(manual.clone()))];
        let mut computers = vec![computer("legacy", Some(manual.clone()))];
        default_new_computer_desktops(&mut computers, &previous, "ubuntu-24.04-v4");
        assert_eq!(configuration(&computers[0]), Some(&manual));
    }

    #[test]
    fn imported_pending_restore_computers_report_stopped_computer_use_and_save_approval() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let configuration = built_in_computer();
        runtime::write_metadata(
            &paths.metadata,
            &runtime::ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![configuration.clone()],
            },
        )
        .unwrap();
        runtime::checkpoints::import_pending_restore(
            &paths,
            configuration.id(),
            "silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9",
            "silo-backup-0-330418-1790360984903",
        )
        .unwrap();
        // No command may reach the runtime: it knows nothing about this computer yet.
        let runner = ScriptedRunner::new([]);
        let resolved = computer_at(&runner, &paths, "dev", None).unwrap();
        assert_eq!(resolved.id(), configuration.id());
        let status = status_with(&runner, &paths, &resolved).unwrap();
        assert_eq!(status["state"], "computer-stopped");
        assert!(status["computerUse"]["state"].is_string());
        let status = approval_at(
            &runner,
            &paths,
            &resolved,
            crate::computer_use::Approval::Auto,
            std::sync::Arc::new(runtime::ProcessRunner),
        )
        .unwrap();
        assert_eq!(status["computerUse"]["approval"], "auto");
        runner.assert_finished();
    }

    #[test]
    fn queued_desktop_action_rejects_a_replacement_computer() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let original = built_in_computer();
        runtime::write_metadata(
            &paths.metadata,
            &runtime::ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![original.clone()],
            },
        )
        .unwrap();
        let mut replacement = original.clone();
        {
            let ComputerConfiguration { id, .. } = &mut replacement;
            *id = "00000000-0000-4000-8000-000000000002".into();
        }
        let runner = ScriptedRunner::new([]);
        let gate = runtime::operation_gate::OperationGate::new();
        let device = gate.device("Replace computer").unwrap();
        std::thread::scope(|scope| {
            let action = scope.spawn(|| {
                prepare_local(&gate, &runner, &paths, "dev", Some("stop"), None)
                    .map(|(_, configuration)| configuration.id().to_owned())
            });
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while gate.snapshot().waiting.is_empty() {
                assert!(std::time::Instant::now() < deadline, "action never queued");
                std::thread::sleep(Duration::from_millis(5));
            }
            assert_eq!(
                gate.snapshot().waiting[0].computer_id.as_deref(),
                Some(original.id())
            );
            runtime::write_metadata(
                &paths.metadata,
                &runtime::ComputerConfigurationRequest {
                    schema_version: 1,
                    computers: vec![replacement],
                },
            )
            .unwrap();
            drop(device);
            let result = action.join().unwrap();
            assert_eq!(
                result.unwrap_err(),
                "The computer changed identity. Refresh before accessing its desktop."
            );
        });
        runner.assert_finished();
        assert!(gate.is_idle());
    }

    #[test]
    fn explicit_desktop_identity_rejects_a_reused_name() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let replacement = built_in_computer();
        runtime::write_metadata(
            &paths.metadata,
            &runtime::ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![replacement],
            },
        )
        .unwrap();
        let gate = runtime::operation_gate::OperationGate::new();
        let runner = ScriptedRunner::new([]);
        let removed_id = "00000000-0000-4000-8000-000000000002";
        for action in [None, Some("stop")] {
            assert!(
                prepare_local(&gate, &runner, &paths, "dev", action, Some(removed_id)).is_err()
            );
        }
        // Approval requests use the same identity check without taking a computer gate.
        assert!(computer_at(&runner, &paths, "dev", Some(removed_id)).is_err());
        assert!(gate.is_idle());
        runner.assert_finished();
    }

    #[test]
    fn desktop_admission_preserves_identity_and_ungated_reads() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let configuration = built_in_computer();
        runtime::write_metadata(
            &paths.metadata,
            &runtime::ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![configuration.clone()],
            },
        )
        .unwrap();
        let labels = json!({"silo.managed":"true","silo.machine-id":configuration.id()});
        let runner = ScriptedRunner::new([
            inspect("Running", labels.clone()),
            inspect("Running", labels),
        ]);
        let gate = runtime::operation_gate::OperationGate::new();
        let (guard, admitted) =
            prepare_local(&gate, &runner, &paths, "dev", Some("stop"), None).unwrap();
        assert_eq!(admitted.id(), configuration.id());
        assert_eq!(
            gate.snapshot().running[0].computer_id.as_deref(),
            Some(configuration.id())
        );
        drop(guard);
        let device = gate.device("Update computer").unwrap();
        let (guard, observed) = prepare_local(
            &gate,
            &runner,
            &paths,
            "dev",
            None,
            Some(configuration.id()),
        )
        .unwrap();
        assert!(guard.is_none());
        assert_eq!(observed.id(), configuration.id());
        drop(device);
        assert!(gate.is_idle());
        runner.assert_finished();
    }

    #[test]
    fn a_real_runtime_computer_must_still_match_its_identity() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let configuration = built_in_computer();
        runtime::write_metadata(
            &paths.metadata,
            &runtime::ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![configuration.clone()],
            },
        )
        .unwrap();
        let runner = ScriptedRunner::new([inspect(
            "Stopped",
            json!({"silo.managed":"true","silo.machine-id":"someone-else"}),
        )]);
        assert!(computer_at(&runner, &paths, "dev", None).is_err());
        runner.assert_finished();
    }

    #[test]
    fn built_in_is_decided_by_silo_not_by_the_request() {
        let claimed = DesktopConfiguration {
            start_with_computer: true,
            built_in: true,
        };
        let plain = DesktopConfiguration {
            start_with_computer: true,
            built_in: false,
        };
        // An existing computer keeps what it had, whatever the saved configuration says.
        let built_in_before = computer("old", Some(claimed.clone()));
        let plain_before = computer("plain", Some(plain.clone()));
        let none_before = computer("none", None);
        let previous = vec![built_in_before, plain_before, none_before];
        let mut computers = vec![
            computer("old", Some(plain.clone())),
            computer("plain", Some(claimed.clone())),
            computer("none", Some(claimed.clone())),
            computer("fresh-v3", Some(claimed.clone())),
        ];
        default_new_computer_desktops(&mut computers, &previous, "ubuntu-24.04-v3");
        assert_eq!(configuration(&computers[0]), Some(&claimed));
        assert_eq!(configuration(&computers[1]), Some(&plain));
        assert_eq!(configuration(&computers[2]), Some(&plain));
        assert_eq!(
            configuration(&computers[3]),
            Some(&plain),
            "pre-v4 images are not built in"
        );
        // The flag is reported as `builtIn` and omitted when false (older UIs and exports).
        assert_eq!(
            serde_json::to_value(&claimed).unwrap(),
            json!({"startWithComputer": true, "builtIn": true})
        );
        assert_eq!(
            serde_json::to_value(&plain).unwrap(),
            json!({"startWithComputer": true})
        );
        let parsed: DesktopConfiguration =
            serde_json::from_value(json!({"startWithComputer": false})).unwrap();
        assert!(!parsed.built_in);
    }

    #[test]
    fn sparse_desktop_settings_keep_legacy_defaults_on_round_trip() {
        for (saved, expected_start) in [
            (json!({}), true),
            (json!({"startWithComputer": false}), false),
        ] {
            let configuration: DesktopConfiguration = serde_json::from_value(saved).unwrap();
            assert_eq!(configuration.start_with_computer, expected_start);
            assert!(!configuration.built_in);
            let encoded = serde_json::to_value(&configuration).unwrap();
            assert_eq!(encoded, json!({"startWithComputer": expected_start}));
            assert_eq!(
                serde_json::from_value::<DesktopConfiguration>(encoded).unwrap(),
                configuration
            );
        }
    }

    #[test]
    fn older_images_keep_the_explicit_install_flow() {
        let mut computers = vec![computer("fresh", None)];
        default_new_computer_desktops(&mut computers, &[], "ubuntu-24.04-v3");
        assert_eq!(configuration(&computers[0]), None);
    }

    fn managed_computer() -> ExpectedCommand {
        inspect("Stopped", json!({"silo.managed":"true"}))
    }

    fn configure_guest(script: String) -> ExpectedCommand {
        ExpectedCommand::ok(
            [
                "exec",
                "dev",
                "--no-tty",
                "--quiet",
                "--timeout",
                "1800s",
                "--user",
                "root",
                "--workdir",
                "/",
                "--",
                "sh",
                "-c",
                &script,
            ],
            "",
        )
        .with_timeout(Duration::from_secs(1860))
    }
    #[test]
    fn install_rejects_runtime_without_boot_hook_before_mutating_guest() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let runner = ScriptedRunner::new([
            managed_computer(),
            ExpectedCommand::ok(["--silo-desktop-protocol"], "0")
                .with_timeout(Duration::from_secs(10)),
        ]);
        let error = configure_with(
            &runner,
            &paths(dir.path()),
            "dev",
            None,
            &DesktopConfiguration {
                start_with_computer: true,
                built_in: false,
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("desktop startup"));
        runner.assert_finished();
    }

    #[test]
    fn changing_startup_policy_does_not_reinstall_or_stop_session() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let runner = ScriptedRunner::new([
            managed_computer(),
            configure_guest("/usr/local/bin/silo-desktop autostart false\n".into()),
        ]);
        configure_with(
            &runner,
            &paths(dir.path()),
            "dev",
            Some(&DesktopConfiguration {
                start_with_computer: true,
                built_in: false,
            }),
            &DesktopConfiguration {
                start_with_computer: false,
                built_in: false,
            },
        )
        .unwrap();
        runner.assert_finished();
    }
    #[test]
    fn fresh_install_stages_agent_tools_before_persisting_startup_policy() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let runner = ScriptedRunner::new([
            managed_computer(),
            ExpectedCommand::ok(["--silo-desktop-protocol"], "1")
                .with_timeout(Duration::from_secs(10)),
            configure_guest(format!(
                "{}/usr/local/bin/silo-desktop autostart false\n",
                installer_script("install")
            )),
        ]);
        configure_with(
            &runner,
            &paths(dir.path()),
            "dev",
            None,
            &DesktopConfiguration {
                start_with_computer: false,
                built_in: false,
            },
        )
        .unwrap();
        runner.assert_finished();
        let calls = runner.calls();
        let script = calls.last().unwrap().last().unwrap();
        assert!(!script.to_lowercase().contains("luda"));
        assert!(script.contains("SILO_DESKTOP_STREAMER_LOCK_SOURCE"));
        assert!(script.contains("desktop-streamer-lock.json"));
        assert!(script.contains("SILO_SELKIES_WEB_CLIENT_PATCH_SOURCE"));
        assert!(script.contains("patch-selkies-web-client.py"));
        assert!(script.contains("set -- install\n"));
        assert!(script.ends_with("/usr/local/bin/silo-desktop autostart false\n"));
    }

    #[test]
    fn streamer_actions_route_to_scoped_guest_commands() {
        let _test_state = crate::test_support::global_state();
        let restart = action_script("restart-streamer");
        assert_eq!(restart, "/usr/local/bin/silo-desktop restart-streamer");
        assert_eq!(action_timeout("restart-streamer"), Duration::from_secs(120));

        let update = action_script("update-streamer");
        assert!(update.contains("SILO_DESKTOP_STREAMER_LOCK_SOURCE"));
        assert!(update.contains("SILO_SELKIES_WEB_CLIENT_PATCH_SOURCE"));
        assert!(update.contains("set -- update-streamer\n"));
        assert_eq!(action_timeout("update-streamer"), Duration::from_secs(1800));
        assert_eq!(
            action_script("restart"),
            "/usr/local/bin/silo-desktop restart"
        );
    }

    #[test]
    fn lcu_setup_is_explicit_staged_and_never_starts_computer() {
        let _test_state = crate::test_support::global_state();
        let setup = action_script("setup-lcu");
        assert!(setup.contains("lcu-lock.json"));
        assert!(setup.contains("/usr/local/share/silo/lcu-lock.json"));
        assert!(setup.contains("/usr/local/libexec/silo-setup-lcu.py setup"));
        assert!(setup.contains("setup-lcu.py\" status"));
        assert!(setup.contains("needs-runtime"));
        assert!(
            setup.find("lcu_status=$(python3").unwrap() < setup.find("install -m 0644").unwrap()
        );
        assert!(!setup.contains("SILO_DESKTOP_LCU_LOCK_SOURCE"));
        assert_eq!(action_timeout("setup-lcu"), Duration::from_secs(1800));
        assert!(!action_starts_computer("setup-lcu"));
        assert!(action_starts_computer("start"));
    }

    #[test]
    fn long_desktop_actions_are_not_flagged_before_their_guest_timeout() {
        let _test_state = crate::test_support::global_state();
        for action in [
            "start",
            "stop",
            "restart",
            "restart-streamer",
            "update-streamer",
            "setup-lcu",
        ] {
            assert!(
                action_expected_duration(action) > action_timeout(action),
                "{action}"
            );
            assert!(action_expected_duration(action) >= Duration::from_secs(10 * 60));
        }
    }

    #[test]
    fn status_bounds_guest_supplied_identity_fields() {
        let _test_state = crate::test_support::global_state();
        let long = "9".repeat(65);
        let status = public_status(json!({
            "installed":true,"autoStart":false,"state":"running",
            "version":long,"user":"silo\u{1b}[31m","display":":1; echo"
        }))
        .unwrap();
        assert!(status["version"].is_null());
        assert!(status["user"].is_null());
        assert!(status["display"].is_null());
        let status = public_status(json!({
            "installed":true,"autoStart":false,"state":"running",
            "version":"1.2.3","user":"silo","display":":1.0"
        }))
        .unwrap();
        assert_eq!(status["version"], "1.2.3");
        assert_eq!(status["user"], "silo");
        assert_eq!(status["display"], ":1.0");
    }

    #[test]
    fn status_projects_only_valid_agent_tool_fields() {
        let _test_state = crate::test_support::global_state();
        // A guest that still reports Luda fields is ignored.
        let status = public_status(json!({"installed":true,"autoStart":false,"state":"stopped","ludaState":"ready","ludaVersion":"0.3.0","ludaError":"private"})).unwrap();
        assert!(status.get("ludaState").is_none());
        assert!(status.get("ludaVersion").is_none());
        assert_eq!(status["backend"], "kasm");
        assert_eq!(status["sessionState"], "stopped");
        assert_eq!(status["streamState"], "stopped");
        assert_eq!(status["updateRequired"], false);
        assert!(!status.to_string().contains("private"));
        let old =
            public_status(json!({"installed":true,"autoStart":false,"state":"stopped"})).unwrap();
        assert_eq!(old["backend"], "kasm");
        assert!(old["lcuState"].is_null());
        let split = public_status(json!({
            "installed":true,"autoStart":true,"state":"failed",
            "backend":"selkies","sessionState":"running","streamState":"failed",
            "updateRequired":true,"streamerVersion":"2.0.0","password":"private"
        }))
        .unwrap();
        assert_eq!(split["backend"], "selkies");
        assert_eq!(split["sessionState"], "running");
        assert_eq!(split["streamState"], "failed");
        assert_eq!(split["updateRequired"], true);
        assert_eq!(split["streamerVersion"], "2.0.0");
        assert!(!split.to_string().contains("private"));
        let lcu = public_status(json!({
            "installed":true,"autoStart":true,"state":"running",
            "sessionState":"running","streamState":"failed",
            "lcuState":"ready","lcuVersion":"0.4.0",
            "lcuAppVersion":"26.924.22138",
            "lcuRuntimeVersion":"0.0.24/20260924074400-f52ea85e2a98",
            "lcuAgents":["pi","codex","claude-code","unexpected"],
            "lcuReadiness":"ready","lcuReason":"invalid-receipt",
            "appPath":"/private/app","password":"private"
        }))
        .unwrap();
        assert_eq!(lcu["state"], "running");
        assert_eq!(lcu["sessionState"], "running");
        assert_eq!(lcu["streamState"], "failed");
        assert_eq!(lcu["lcuState"], "ready");
        assert_eq!(lcu["lcuAppVersion"], "26.924.22138");
        assert_eq!(
            lcu["lcuRuntimeVersion"],
            "0.0.24/20260924074400-f52ea85e2a98"
        );
        assert_eq!(lcu["lcuAgents"], json!(["pi", "codex", "claude-code"]));
        assert!(!lcu.to_string().contains("/private/app"));
        assert!(!lcu.to_string().contains("private"));
        let unsafe_runtime = public_status(json!({
            "installed":true,"autoStart":true,"state":"running",
            "lcuRuntimeVersion":"../../private/runtime"
        }))
        .unwrap();
        assert!(unsafe_runtime["lcuRuntimeVersion"].is_null());
        let prerequisite = public_status(json!({
            "installed":true,"autoStart":true,"state":"running",
            "lcuState":"needs-runtime","lcuReason":"chatgpt-app-required"
        }))
        .unwrap();
        assert_eq!(prerequisite["state"], "running");
        assert_eq!(prerequisite["lcuState"], "needs-runtime");
        assert_eq!(prerequisite["lcuReason"], "chatgpt-app-required");
    }

    struct ConnectionRunner {
        configuration: ComputerConfiguration,
        calls: std::sync::Mutex<Vec<Vec<String>>>,
        on_connection: Option<Box<dyn Fn(&RuntimePaths) + Send + Sync>>,
    }
    impl RuntimeRunner for ConnectionRunner {
        fn run(
            &self,
            paths: &RuntimePaths,
            args: &[String],
            _: Duration,
        ) -> Result<runtime::CommandOutput, RuntimeError> {
            self.calls.lock().unwrap().push(args.to_vec());
            let stdout = match args.first().map(String::as_str) {
                Some("inspect") => json!({
                    "name":self.configuration.name(),"status":"Running",
                    "config":{"labels":{"silo.managed":"true","silo.machine-id":self.configuration.id()}}
                })
                .to_string(),
                Some("exec")
                    if args.last().map(String::as_str)
                        == Some("/usr/local/bin/silo-desktop connection") =>
                {
                    if let Some(change) = &self.on_connection {
                        change(paths);
                    }
                    json!({"port":6901,"username":"silo","password":"b".repeat(64)}).to_string()
                }
                Some("exec")
                    if args.last().is_some_and(|script| {
                        script.contains("/usr/local/bin/silo-desktop status")
                    }) =>
                {
                    format!(
                        "{}\n{{}}\n",
                        json!({"installed":true,"state":"running","autoStart":true,"backend":"selkies"})
                    )
                }
                _ => panic!("Unexpected desktop connection command: {args:?}"),
            };
            Ok(runtime::CommandOutput {
                stdout,
                stderr: String::new(),
            })
        }
    }

    #[test]
    fn desktop_connection_rejects_a_reused_name_for_an_explicit_computer_id() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let replacement = built_in_computer();
        runtime::write_metadata(
            &paths.metadata,
            &runtime::ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![replacement.clone()],
            },
        )
        .unwrap();
        let runner = ConnectionRunner {
            configuration: replacement,
            calls: Default::default(),
            on_connection: None,
        };
        let removed_id = "00000000-0000-4000-8000-000000000002";
        let result = connection_with(&runner, &paths, "dev", Some(removed_id));
        assert!(
            result.is_err(),
            "returned replacement credentials for the removed computer"
        );
        assert!(runner.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn desktop_connection_discards_credentials_if_the_computer_changes_during_lookup() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let original = built_in_computer();
        runtime::write_metadata(
            &paths.metadata,
            &runtime::ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![original.clone()],
            },
        )
        .unwrap();
        let mut replacement = original.clone();
        {
            let ComputerConfiguration { id, .. } = &mut replacement;
            *id = "00000000-0000-4000-8000-000000000002".into();
        }
        let runner = ConnectionRunner {
            configuration: original.clone(),
            calls: Default::default(),
            on_connection: Some(Box::new(move |paths| {
                let _change = runtime::OPERATIONS
                    .device("Replace fixture computer")
                    .unwrap();
                runtime::write_metadata(
                    &paths.metadata,
                    &runtime::ComputerConfigurationRequest {
                        schema_version: 1,
                        computers: vec![replacement.clone()],
                    },
                )
                .unwrap();
            })),
        };
        let result = connection_with(&runner, &paths, "dev", Some(original.id()));
        assert!(
            result.is_err(),
            "returned credentials after the selected computer was replaced"
        );
        assert_ne!(
            runtime::resolve_computer_id(&paths, "dev").unwrap(),
            original.id()
        );
    }

    #[test]
    fn desktop_connection_returns_credentials_for_the_matching_computer() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let configuration = built_in_computer();
        runtime::write_metadata(
            &paths.metadata,
            &runtime::ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![configuration.clone()],
            },
        )
        .unwrap();
        for expected in [None, Some(configuration.id())] {
            let runner = ConnectionRunner {
                configuration: configuration.clone(),
                calls: Default::default(),
                on_connection: None,
            };
            let result = connection_with(&runner, &paths, "dev", expected).unwrap();
            assert_eq!(
                result,
                json!({"port":6901,"username":"silo","password":"b".repeat(64)})
            );
            let calls = runner.calls.lock().unwrap();
            assert_eq!(calls.len(), 5);
            assert!(calls
                .iter()
                .filter(|args| args[0] == "exec")
                .all(|args| args.iter().any(|arg| arg == "--no-start")));
        }
    }

    #[test]
    fn desktop_status_rejects_a_runtime_replaced_after_resolution() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let configuration = built_in_computer();
        runtime::write_metadata(
            &paths.metadata,
            &runtime::ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![configuration.clone()],
            },
        )
        .unwrap();
        for status in ["Stopped", "Running"] {
            let runner = ScriptedRunner::new([
                inspect(
                    "Running",
                    json!({"silo.managed":"true","silo.machine-id":configuration.id()}),
                ),
                inspect(
                    status,
                    json!({"silo.managed":"true","silo.machine-id":"replacement"}),
                ),
            ]);
            let resolved = computer_at(&runner, &paths, "dev", Some(configuration.id())).unwrap();
            assert!(
                status_with(&runner, &paths, &resolved).is_err(),
                "accepted replacement runtime"
            );
            runner.assert_finished();
        }
    }

    #[test]
    fn desktop_status_rejects_unmanaged_or_renamed_runtime() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let configuration = built_in_computer();
        for (name, managed) in [("dev", "false"), ("other", "true")] {
            let runner = ScriptedRunner::new([ExpectedCommand::ok(
                ["inspect", "dev", "--format", "json"],
                json!({"name":name,"status":"Stopped","config":{"labels":{
                    "silo.managed":managed,"silo.machine-id":configuration.id()
                }}})
                .to_string(),
            )]);
            assert!(status_with(&runner, &paths(dir.path()), &configuration).is_err());
            runner.assert_finished();
        }
    }

    #[test]
    fn desktop_approval_rejects_a_replacement_without_saving_policy() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let configuration = built_in_computer();
        let labels = json!({"silo.managed":"true","silo.machine-id":"replacement"});
        let runner = ScriptedRunner::new([inspect("Stopped", labels)]);
        let result = approval_at(
            &runner,
            &paths,
            &configuration,
            crate::computer_use::Approval::Auto,
            std::sync::Arc::new(runtime::ProcessRunner),
        );
        assert!(result.is_err(), "saved approval for a replaced runtime");
        assert_eq!(
            crate::computer_use::settings(&paths, configuration.id()).approval,
            crate::computer_use::Approval::Ask
        );
        runner.assert_finished();
    }

    #[test]
    fn stopped_status_never_boots_computer() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let runner = ScriptedRunner::new([inspect(
            "Stopped",
            json!({"silo.managed":"true","silo.machine-id":"id"}),
        )]);
        let configuration: ComputerConfiguration = serde_json::from_value(json!({"id":"id","name":"dev","cpus":1,"maxCPUs":1,"memoryGiB":2,"maxMemoryGiB":2,"workspaceStorageGiB":10,"runtimeStorageGiB":10,"desktop":{"startWithComputer":false}})).unwrap();
        assert_eq!(
            status_with(&runner, &paths(dir.path()), &configuration).unwrap(),
            json!({"installed":true,"state":"computer-stopped","autoStart":false,
                   "backend":null,"sessionState":"stopped","streamState":"stopped",
                   "updateRequired":false,"updateAvailable":false,"streamerVersion":null,
                   "lcuState":null,"lcuReason":null,"lcuVersion":null,
                   "lcuAppVersion":null,"lcuRuntimeVersion":null,
                   "lcuAgents":null,"lcuReadiness":null})
        );
        runner.assert_finished();
    }
    fn receipt_read_suffix() -> String {
        format!("\nprintf '%s\\n' \"$(tr -d '\\n\\r' < {STREAMER_RECEIPT} 2>/dev/null || true)\"\n")
    }

    fn recipe_status(helper: Value, receipt: &str) -> Value {
        let dir = tempfile::tempdir().unwrap();
        let configuration: ComputerConfiguration = serde_json::from_value(json!({"id":"id","name":"dev","cpus":1,"maxCPUs":1,"memoryGiB":2,"maxMemoryGiB":2,"workspaceStorageGiB":10,"runtimeStorageGiB":10,"desktop":{"startWithComputer":true}})).unwrap();
        let script = format!(
            "if [ -x /usr/local/bin/silo-desktop ]; then /usr/local/bin/silo-desktop status; else printf '%s\\n' '{{\"installed\":false,\"state\":\"uninstalled\",\"autoStart\":false}}'; fi{}",
            receipt_read_suffix()
        );
        let runner = ScriptedRunner::new([
            inspect(
                "Running",
                json!({"silo.managed":"true","silo.machine-id":"id"}),
            ),
            ExpectedCommand::ok(
                [
                    "exec",
                    "dev",
                    "--no-start",
                    "--no-tty",
                    "--quiet",
                    "--timeout",
                    "15s",
                    "--user",
                    "root",
                    "--workdir",
                    "/",
                    "--",
                    "sh",
                    "-c",
                    script.as_str(),
                ],
                format!("{helper}\n{receipt}\n"),
            ),
        ]);
        let status = status_with(&runner, &paths(dir.path()), &configuration).unwrap();
        runner.assert_finished();
        status
    }

    #[test]
    fn an_older_helper_cannot_hide_or_force_a_desktop_update() {
        let _test_state = crate::test_support::global_state();
        let bundled = bundled_recipe_version().unwrap();
        let old_helper = |required: Value| {
            let mut value = json!({"installed":true,"state":"stopped","autoStart":true,"backend":"selkies","sessionState":"stopped","streamState":"stopped","streamerVersion":"2.0.0"});
            if !required.is_null() {
                value["updateRequired"] = required;
            }
            value
        };
        let older = format!(
            "{{\"backend\":\"selkies\",\"recipeVersion\":{}}}",
            bundled - 1
        );
        for required in [Value::Null, json!(false), json!(true)] {
            let status = recipe_status(old_helper(required), &older);
            assert_eq!(status["updateAvailable"], true);
            assert_eq!(status["updateRequired"], false);
        }
        let current = format!("{{\"backend\":\"selkies\",\"recipeVersion\":{bundled}}}");
        let status = recipe_status(old_helper(json!(true)), &current);
        assert_eq!(status["updateAvailable"], false);
        assert_eq!(status["updateRequired"], false);
        let status = recipe_status(old_helper(json!(true)), "");
        assert_eq!(status["updateRequired"], true);
        assert_eq!(status["updateAvailable"], false);
        let status = recipe_status(
            old_helper(json!(true)),
            "{\"backend\":\"selkies\",\"recipeVersion\":0}",
        );
        assert_eq!(status["updateRequired"], true);
        assert_eq!(status["updateAvailable"], false);
    }

    fn built_in_computer() -> ComputerConfiguration {
        serde_json::from_value(json!({"id":"00000000-0000-4000-8000-000000000001","name":"dev","cpus":1,"maxCPUs":1,"memoryGiB":2,"maxMemoryGiB":2,"workspaceStorageGiB":10,"runtimeStorageGiB":10,"desktop":{"startWithComputer":true,"builtIn":true}})).unwrap()
    }

    #[test]
    fn a_stopped_built_in_computer_still_reports_computer_use_without_booting() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        crate::computer_use::set_approval(
            &paths,
            built_in_computer().id(),
            crate::computer_use::Approval::Auto,
        )
        .unwrap();
        let runner = ScriptedRunner::new([inspect(
            "Stopped",
            json!({"silo.managed":"true","silo.machine-id":built_in_computer().id()}),
        )]);
        let status = status_with(&runner, &paths, &built_in_computer()).unwrap();
        assert_eq!(status["state"], "computer-stopped");
        assert_eq!(status["computerUse"]["approval"], "auto");
        assert!(status["computerUse"]["state"].is_string());
        runner.assert_finished();
        // A computer without built-in computer use reports none.
        let runner = ScriptedRunner::new([inspect(
            "Stopped",
            json!({"silo.managed":"true","silo.machine-id":"id"}),
        )]);
        let configuration: ComputerConfiguration = serde_json::from_value(json!({"id":"id","name":"dev","cpus":1,"maxCPUs":1,"memoryGiB":2,"maxMemoryGiB":2,"workspaceStorageGiB":10,"runtimeStorageGiB":10,"desktop":{"startWithComputer":true}})).unwrap();
        assert!(status_with(&runner, &paths, &configuration)
            .unwrap()
            .get("computerUse")
            .is_none());
    }

    #[test]
    fn a_running_built_in_computer_reads_both_statuses_in_one_guest_command() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let configuration = built_in_computer();
        let script = format!(
            "if [ -x /usr/local/bin/silo-desktop ]; then /usr/local/bin/silo-desktop status; else printf '%s\\n' '{{\"installed\":false,\"state\":\"uninstalled\",\"autoStart\":false}}'; fi\n{}{}",
            crate::computer_use::STATUS_COMMAND,
            receipt_read_suffix()
        );
        let guest_output = format!(
            "{}\n{}\n",
            json!({"installed":true,"state":"running","autoStart":true,"sessionState":"running","streamState":"running","backend":"selkies"}),
            json!({"state":"ready","reason":null,"compatibility":"untested","warning":"Not tested.","appVersion":"26.928.31416","runtimeVersion":null,"lcuVersion":"0.8.0","agents":["codex"],"approval":"ask","mount":"ok"})
        ) + "{\"backend\":\"selkies\",\"recipeVersion\":2}\n";
        let runner = ScriptedRunner::new([
            inspect(
                "Running",
                json!({"silo.managed":"true","silo.machine-id":configuration.id()}),
            ),
            ExpectedCommand::ok(
                [
                    "exec",
                    "dev",
                    "--no-start",
                    "--no-tty",
                    "--quiet",
                    "--timeout",
                    "15s",
                    "--user",
                    "root",
                    "--workdir",
                    "/",
                    "--",
                    "sh",
                    "-c",
                    script.as_str(),
                ],
                guest_output,
            ),
        ]);
        crate::chatgpt_app::set_test_cache(Some(crate::chatgpt_app::Status::Ready {
            path: "/x".into(),
            version: "26.928.31416".into(),
        }));
        let status = status_with(&runner, &paths(dir.path()), &configuration).unwrap();
        crate::chatgpt_app::set_test_cache(None);
        assert_eq!(status["state"], "running");
        let computer_use = &status["computerUse"];
        assert_eq!(computer_use["state"], "ready");
        assert_eq!(computer_use["compatibility"], "untested");
        assert_eq!(computer_use["warning"], "Not tested.");
        assert_eq!(computer_use["lcuVersion"], "0.8.0");
        assert_eq!(computer_use["agents"], json!(["codex"]));
        assert_eq!(computer_use["approval"], "ask");
        runner.assert_finished();
    }

    #[test]
    fn setup_computer_use_is_a_known_action_only_for_built_in_computers() {
        assert_eq!(
            action_timeout("setup-computer-use"),
            Duration::from_secs(1800)
        );
        assert!(action_expected_duration("setup-computer-use") >= Duration::from_secs(1800));
        assert!(!action_starts_computer("setup-computer-use"));
    }

    #[test]
    fn status_projection_does_not_leak_guest_credentials() {
        let _test_state = crate::test_support::global_state();
        let public = public_status(json!({"installed":true,"autoStart":true,"state":"running","password":"private","connection":{"token":"private"}})).unwrap();
        assert!(!public.to_string().contains("private"));
        assert!(
            public_status(json!({"installed":true,"autoStart":true,"state":"surprise"})).is_err()
        );
    }
}
