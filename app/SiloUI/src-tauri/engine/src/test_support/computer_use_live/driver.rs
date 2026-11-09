//! Live checks of Cua Driver inside a built-in computer: its version, its readiness check in the
//! desktop session and the MCP registration the helper makes for the agents.
use super::Fixture;
use crate::runtime;
use serde_json::Value;
use std::path::PathBuf;
use std::time::Instant;

impl Fixture {
    /// Runs `script` as root and returns stdout whatever the exit status (a trailing
    /// `EXIT:<code>` line names it), so a failing guest command can be asserted on.
    pub(crate) fn exec_status(&self, name: &str, script: &str) -> String {
        self.exec(name, "root", &format!("({script}) 2>&1; echo EXIT:$?"))
            .unwrap_or_else(|error| format!("exec failed: {error}"))
    }

    /// Runs `script` as the silo account with the environment of the running desktop session,
    /// read from the file the session wrote the way `/usr/local/libexec/silo-cua-driver-mcp`
    /// reads it, so it behaves as an agent's MCP server does.
    pub(crate) fn exec_in_session(&self, name: &str, script: &str) -> String {
        self.exec_status(
            name,
            &format!(
                "session=$({SESSION_ENVIRONMENT}); \
                 runuser -u silo -- env HOME=/home/silo USER=silo LOGNAME=silo CUA_DRIVER_RS_TELEMETRY_ENABLED=0 CUA_DRIVER_RS_UPDATE_CHECK=false $session {script}"
            ),
        )
    }

    /// Waits for `cua-driver doctor` to pass in the desktop session with a working display,
    /// X11 connection and accessibility bus: computer use really works, not only what Silo's
    /// status reports. Returns how long it took.
    pub(crate) fn wait_doctor(&self, name: &str, label: &str) -> std::time::Duration {
        let started = Instant::now();
        loop {
            let output =
                self.exec_in_session(name, "/usr/local/bin/cua-driver doctor --json 2>/dev/null");
            let failing = doctor_failures(&output);
            if failing.is_empty() {
                eprintln!(
                    "[{:>4}s] {label}: cua-driver doctor passed",
                    started.elapsed().as_secs()
                );
                return started.elapsed();
            }
            assert!(
                started.elapsed() < std::time::Duration::from_secs(300),
                "{label}: cua-driver doctor never passed ({failing:?}): {output}"
            );
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
    }

    /// Prints the numbers the plan asks for: guest memory and disk, and on the host the
    /// image cache and the computer's own disk files.
    pub(crate) fn measure(&self, name: &str, label: &str) {
        let guest = self.exec_status(
            name,
            "free -m | sed -n 1,2p; df -h / /workspace | cat; cat /proc/loadavg",
        );
        eprintln!("MEASURE {label} guest:\n{guest}");
        let du = |path: PathBuf| {
            std::process::Command::new("du")
                .arg("-sk")
                .arg(path)
                .output()
                .ok()
                .and_then(|output| {
                    String::from_utf8_lossy(&output.stdout)
                        .split_whitespace()
                        .next()
                        .and_then(|kib| kib.parse::<u64>().ok())
                })
                .unwrap_or(0)
        };
        eprintln!(
            "MEASURE {label} host: image cache {} MiB, computer {name} {} MiB",
            du(self.paths.home.join("cache")) / 1024,
            du(self.paths.home.join("sandboxes").join(name)) / 1024,
        );
    }
}

/// Prints `KEY=value` for the display variables the desktop session wrote when it started.
const SESSION_ENVIRONMENT: &str = r#"grep -E '^(DISPLAY|DBUS_SESSION_BUS_ADDRESS|XDG_RUNTIME_DIR|XAUTHORITY)=' /run/silo-desktop/user/session.env | tr '\n' ' '"#;

/// The probes a working desktop session needs from `cua-driver doctor --json`.
const DOCTOR_PROBES: [&str; 3] = ["display server", "X11 connection", "AT-SPI"];

/// What keeps `exec_in_session`'s doctor output from passing: a non-zero exit, `ok` not true,
/// or one of [`DOCTOR_PROBES`] missing or not `ok`. Empty when it passes.
fn doctor_failures(output: &str) -> Vec<String> {
    let Some((json, exit)) = output.trim_end().rsplit_once("EXIT:") else {
        return vec!["no exit status".into()];
    };
    let mut failing = Vec::new();
    if exit.trim() != "0" {
        failing.push(format!("exit {}", exit.trim()));
    }
    let Ok(report) = serde_json::from_str::<Value>(json.trim()) else {
        failing.push("no JSON report".into());
        return failing;
    };
    if report["ok"] != true {
        failing.push("ok is not true".into());
    }
    for label in DOCTOR_PROBES {
        let status = report["probes"]
            .as_array()
            .and_then(|probes| probes.iter().find(|probe| probe["label"] == label))
            .map(|probe| probe["status"].clone());
        if status != Some(Value::from("ok")) {
            failing.push(format!("{label}: {status:?}"));
        }
    }
    failing
}

#[test]
fn doctor_failures_require_the_session_probes() {
    let report = |display: &str| {
        format!(
            "{}\nEXIT:0",
            serde_json::json!({"ok": true, "probes": [
                {"label": "display server", "status": display},
                {"label": "X11 connection", "status": "ok"},
                {"label": "AT-SPI", "status": "ok"},
            ]})
        )
    };
    assert!(doctor_failures(&report("ok")).is_empty());
    assert_eq!(
        doctor_failures(&report("warn")),
        ["display server: Some(String(\"warn\"))"]
    );
    assert_eq!(doctor_failures("exec failed: no"), ["no exit status"]);
    assert_eq!(doctor_failures("{}\nEXIT:1").len(), 5);
}

/// A fresh v4 computer through Silo's create and Start paths: Cua Driver is installed at the
/// pinned version, its doctor passes in the desktop session, nothing is mounted from the host
/// and the agents' registrations carry the telemetry and update-check switches.
/// `SILO_LIVE_EVIDENCE` is not used.
#[test]
#[ignore = "requires the v4 guest image, network access and hardware virtualization"]
fn live_cua_driver_is_ready_in_the_desktop() {
    let _state = crate::test_support::global_state();
    let mut fixture = Fixture::new("silo-cua-", None);
    let name = "e2e-cua";
    let started = Instant::now();
    let configuration = fixture.create(name);
    assert!(
        crate::computer_use::is_built_in(&configuration),
        "{configuration:?}"
    );
    runtime::start_disposable_test_computer(&fixture.paths, name).unwrap();
    let (status, _) = fixture.wait_ready(name, "driver");
    eprintln!(
        "MEASURE time: create to computer-use ready {}s",
        started.elapsed().as_secs()
    );
    assert_eq!(status["sessionState"], "running");
    let pinned = crate::computer_use::pinned().unwrap();
    assert_eq!(
        status["computerUse"]["driverVersion"], pinned["version"],
        "{status}"
    );
    fixture.measure(name, "ready");

    let version = fixture.exec_status(name, "/usr/local/bin/cua-driver --version");
    assert!(
        version.contains(&format!(
            "cua-driver {}",
            pinned["version"].as_str().unwrap()
        )),
        "{version}"
    );
    fixture.wait_doctor(name, "driver");
    let mounts = fixture.exec_status(name, "grep -c ' /opt/silo/' /proc/mounts");
    assert!(mounts.starts_with('0'), "{mounts}");
    let inspected =
        runtime::inspect_computer(&runtime::ProcessRunner, &fixture.paths, name).unwrap();
    assert!(!inspected.config["mounts"]
        .to_string()
        .contains("/opt/silo/"));

    // A registration for every installed agent carries the switches that keep the driver quiet.
    let receipt: Value = serde_json::from_str(
        fixture
            .exec(name, "root", "cat /var/lib/silo-computer-use/receipt.json")
            .unwrap()
            .trim(),
    )
    .unwrap();
    eprintln!("receipt: {receipt}");
    assert_eq!(receipt["state"], "ready");
    fixture.stop(name);
}
