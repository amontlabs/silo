//! Live checks of LCU inside a built-in computer: its status, doctor and MCP client driving the
//! desktop without a model, and the approval switch editing agent configuration.
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

    /// Runs `script` inside the silo account's desktop session, the way Silo's helper does.
    pub(crate) fn exec_in_session(&self, name: &str, script: &str) -> String {
        self.exec_status(
            name,
            &format!(
                "runuser -u silo -- env HOME=/home/silo USER=silo LOGNAME=silo /opt/lcu/current/bin/lcu-session --user silo -- {script}"
            ),
        )
    }

    /// Waits for `lcu doctor` (window list and a screenshot inside the desktop session) to
    /// pass: computer use really works, not only what Silo's status reports. Returns how
    /// long it took.
    pub(crate) fn wait_doctor(&self, name: &str, label: &str) -> std::time::Duration {
        let started = Instant::now();
        loop {
            let doctor = self.exec_in_session(
                name,
                "/opt/lcu/current/bin/lcu doctor --non-interactive --require-ready",
            );
            if doctor.contains("Computer use is ready") && doctor.contains("EXIT:0") {
                eprintln!(
                    "[{:>4}s] {label}: lcu doctor ready",
                    started.elapsed().as_secs()
                );
                return started.elapsed();
            }
            assert!(
                started.elapsed() < std::time::Duration::from_secs(300),
                "{label}: lcu doctor never passed: {doctor}"
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

const DRIVER: &str = include_str!("drive.mjs");

/// Pushes the LCU driver into the guest and runs it in the desktop session; `env` is
/// extra `KEY=value` assignments. Returns the driver's output and exit line.
fn run_driver(fixture: &Fixture, name: &str, env: &str) -> String {
    fixture.exec_status(
        name,
        &format!(
            "cat > /tmp/drive.mjs <<'DRIVER_EOF'\n{DRIVER}\nDRIVER_EOF\nchmod 644 /tmp/drive.mjs\nrunuser -u silo -- env {env} HOME=/home/silo USER=silo LOGNAME=silo /opt/lcu/current/bin/lcu-session --user silo -- /opt/lcu/current/agent-tools/node/bin/node /tmp/drive.mjs"
        ),
    )
}

fn mark<'a>(output: &'a str, name: &str) -> Option<&'a str> {
    output
        .lines()
        .find_map(|line| line.strip_prefix(&format!("MARK {name} ")))
}

/// A fresh v4 computer through Silo's create and Start paths, then LCU's own MCP client drives
/// the desktop with no model: window list, screenshot, GNOME Text Editor (GTK4: typeText
/// and paste must not crash it) saved through its dialog, and per-key typing into a
/// terminal. Files are verified from outside the driver. Also records the numbers of the
/// plan: time to ready, memory, disk. `SILO_LIVE_EVIDENCE` names a directory that receives
/// the screenshots the driver took.
#[test]
#[ignore = "requires the v4 guest image, a published ChatGPT app and hardware virtualization"]
fn live_lcu_drives_the_desktop_without_a_model() {
    let _state = crate::test_support::global_state();
    let mut fixture = Fixture::new("silo-lcu-", None, true);
    let name = "e2e-lcu";
    let started = Instant::now();
    let configuration = fixture.create(name);
    assert!(
        crate::computer_use::is_built_in(&configuration),
        "{configuration:?}"
    );
    let created = started.elapsed();
    runtime::start_disposable_test_computer(&fixture.paths, name).unwrap();
    let (status, _) = fixture.wait_ready(name, "drive");
    let ready = started.elapsed();
    eprintln!(
        "MEASURE time: create {}s, create to computer-use ready {}s",
        created.as_secs(),
        ready.as_secs()
    );
    assert_eq!(status["computerUse"]["compatibility"], "tested");
    assert_eq!(status["sessionState"], "running");
    fixture.measure(name, "ready");

    // `lcu status --json` and the doctor, as Silo's own helper runs them.
    let report = fixture.exec_status(
        name,
        "runuser -u silo -- env HOME=/home/silo /opt/lcu/current/bin/lcu status --json",
    );
    eprintln!("lcu status: {report}");
    let report: Value = serde_json::from_str(report.split("\nEXIT:").next().unwrap()).unwrap();
    assert_eq!(report["compatibility"]["status"], "tested");
    assert_eq!(report["lcu_version"], "0.9.4");
    let doctor = fixture.exec_in_session(
        name,
        "/opt/lcu/current/bin/lcu doctor --non-interactive --require-ready",
    );
    eprintln!("lcu doctor: {doctor}");
    assert!(doctor.contains("Computer use is ready"), "{doctor}");
    assert!(doctor.contains("EXIT:0"), "{doctor}");
    let mounts = fixture.exec_status(
        name,
        "grep ' /opt/silo/chatgpt ' /proc/mounts; touch /opt/silo/chatgpt/x",
    );
    assert!(mounts.contains(" ro,"), "{mounts}");
    assert!(mounts.contains("Read-only file system"), "{mounts}");

    // Accessibility is on without any app asking: the system default and the poller that
    // makes Chromium and Electron expose their trees. No browser ships in the image.
    let accessibility = fixture.exec_status(
        name,
        "grep -rh toolkit-accessibility /etc/dconf; pgrep -fc 'silo-accessibility'; \
         which firefox chromium chromium-browser google-chrome || echo no-browser-installed",
    );
    eprintln!("accessibility: {accessibility}");
    assert!(
        accessibility.contains("toolkit-accessibility=true"),
        "{accessibility}"
    );
    assert!(!accessibility.contains("\n0\n"), "{accessibility}");

    // LCU 0.8.2 supplies Codex's disabled sandbox-state meta by default, so even a bare
    // MCP client (no `_meta` at all) reaches the X server through the `js` tool. Silo must
    // not set `LCU_NODE_REPL_SANDBOX=host`.
    let bare = run_driver(&fixture, name, "DRIVE_MODE=bare");
    eprintln!("{bare}");
    assert_eq!(mark(&bare, "bare-denied"), Some("no"), "{bare}");
    assert_eq!(mark(&bare, "bare-windows"), Some("yes"), "{bare}");
    assert_eq!(mark(&bare, "bare-screenshot"), Some("yes"), "{bare}");
    let probe = run_driver(&fixture, name, "DRIVE_MODE=probe");
    assert_eq!(mark(&probe, "x11-reachable"), Some("yes"), "{probe}");

    // The drive itself, in LCU's default configuration (node_repl computer untouched).
    let output = run_driver(&fixture, name, "DRIVE=default");
    eprintln!("{output}");
    assert_eq!(mark(&output, "x11-reachable"), Some("yes"), "{output}");
    assert_eq!(mark(&output, "editor-window"), Some("found"), "{output}");
    assert_eq!(
        mark(&output, "editor-processes-after-text"),
        Some("1"),
        "{output}"
    );
    assert_eq!(mark(&output, "save-dialog"), Some("found"), "{output}");
    assert_eq!(
        mark(&output, "editor-processes-after-save"),
        Some("1"),
        "{output}"
    );
    assert_eq!(
        mark(&output, "editor-processes-after-keys"),
        Some("1"),
        "{output}"
    );
    assert_eq!(mark(&output, "terminal-window"), Some("found"), "{output}");
    assert_eq!(mark(&output, "done"), Some("yes"), "{output}");

    // Independent verification: separate guest commands read what the driver caused.
    let saved = fixture.exec_status(
        name,
        "od -c /home/silo/e2e-lcu-out*.txt | head -5; sha256sum /home/silo/e2e-lcu-out*",
    );
    eprintln!("saved file:\n{saved}");
    let text = fixture
        .exec(name, "root", "cat /tmp/e2e-before-keys.txt")
        .unwrap();
    assert_eq!(
        text.trim_end_matches('\n'),
        "typed-by-typeText\npasted-text caf\u{e9}\nline2",
        "{text:?}"
    );
    // LCU 0.8.3 translates window-targeted keys for GTK 4: ctrl+a, BackSpace, per-key typing
    // and ctrl+s replaced the saved file's bytes (the old text is gone).
    let after_keys = fixture
        .exec(name, "root", "cat /home/silo/e2e-lcu-out*.txt")
        .unwrap();
    // GNOME Text Editor appends a final newline when saving, as for the earlier save.
    assert_eq!(
        after_keys.trim_end_matches('\n'),
        "keys-ok",
        "{after_keys:?}"
    );
    let perkey = fixture
        .exec(name, "root", "cat /tmp/e2e-perkey.txt")
        .unwrap();
    assert_eq!(perkey, "perkey-ok\n");
    // No crash: the editor kept running (checked by the driver) and the kernel logged
    // no fault for it.
    let faults = fixture.exec_status(name, "dmesg | grep -i segfault | head -3");
    assert!(!faults.to_lowercase().contains("segfault"), "{faults}");
    fixture.measure(name, "after-drive");
    if let Ok(evidence) = std::env::var("SILO_LIVE_EVIDENCE") {
        for (file, target) in [
            ("e2e-shot-typed.png", "lcu-editor-typed.png.b64"),
            ("e2e-shot-terminal.png", "lcu-terminal-perkey.png.b64"),
        ] {
            if let Ok(encoded) = fixture.exec(name, "root", &format!("base64 /tmp/{file}")) {
                let _ = std::fs::write(PathBuf::from(&evidence).join(target), encoded);
            }
        }
    }
    fixture.stop(name);
}
