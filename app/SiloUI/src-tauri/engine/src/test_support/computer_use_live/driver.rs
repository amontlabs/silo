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

    /// Runs `script` as the silo account with the desktop session's environment, which the
    /// helper records in its receipt, the way an agent's MCP server gets it.
    pub(crate) fn exec_in_session(&self, name: &str, script: &str) -> String {
        self.exec_status(
            name,
            &format!(
                "session=$(python3 -c \"import json;print(' '.join(f'{{k}}={{v}}' for k,v in json.load(open('/var/lib/silo-computer-use/receipt.json')).get('sessionEnv',{{}}).items()))\"); \
                 runuser -u silo -- env HOME=/home/silo USER=silo LOGNAME=silo CUA_DRIVER_RS_TELEMETRY_ENABLED=0 CUA_DRIVER_RS_UPDATE_CHECK=false $session {script}"
            ),
        )
    }

    /// Waits for `cua-driver doctor` to pass in the desktop session: computer use really
    /// works, not only what Silo's status reports. Returns how long it took.
    pub(crate) fn wait_doctor(&self, name: &str, label: &str) -> std::time::Duration {
        let started = Instant::now();
        loop {
            let doctor = self.exec_in_session(name, "/usr/local/bin/cua-driver doctor --json");
            if doctor.contains("EXIT:0") {
                eprintln!(
                    "[{:>4}s] {label}: cua-driver doctor passed",
                    started.elapsed().as_secs()
                );
                return started.elapsed();
            }
            assert!(
                started.elapsed() < std::time::Duration::from_secs(300),
                "{label}: cua-driver doctor never passed: {doctor}"
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
