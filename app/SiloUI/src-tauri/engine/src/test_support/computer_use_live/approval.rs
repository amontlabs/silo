//! Live check of the per-computer approval switch against real Codex and Claude Code installs.
use super::Fixture;
use crate::runtime;
use serde_json::Value;

const INSTALL_HARNESS_PACKAGES: &str = r#"install_output=$(npm install -g --prefix /usr/local @openai/codex @anthropic-ai/claude-code 2>&1) || {
    printf '%s\n' "$install_output" | tail -2
    exit 1
}
printf '%s\n' "$install_output" | tail -2
"#;

#[test]
fn harness_package_install_rejects_npm_failure() {
    let directory = tempfile::tempdir().unwrap();
    crate::test_support::write_shell_script(
        &directory.path().join("npm"),
        "printf 'install failed\\n' >&2; exit 23",
    );
    let output = std::process::Command::new("/bin/sh")
        .args([
            "-c",
            &format!("set -e; {INSTALL_HARNESS_PACKAGES}; printf 'accepted\\n'"),
        ])
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", directory.path().display()),
        )
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("accepted"));
    assert!(String::from_utf8_lossy(&output.stdout).contains("install failed"));
}

#[test]
fn harness_package_install_keeps_successful_output_tail() {
    let directory = tempfile::tempdir().unwrap();
    crate::test_support::write_shell_script(
        &directory.path().join("npm"),
        "printf 'progress\\ninstalled\\nready\\n'",
    );
    let output = std::process::Command::new("/bin/sh")
        .args(["-c", &format!("set -e; {INSTALL_HARNESS_PACKAGES}")])
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", directory.path().display()),
        )
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"installed\nready\n");
}

/// Reads a guest file as root; empty when it does not exist.
fn guest_file(fixture: &Fixture, name: &str, path: &str) -> String {
    fixture
        .exec(name, "root", &format!("cat {path} 2>/dev/null; true"))
        .unwrap_or_default()
}

/// Installs Node (the build for the guest's own architecture, from `uname -m` inside the
/// guest), Codex and Claude Code in the guest from the public npm registry.
fn install_harnesses(fixture: &Fixture, name: &str) -> bool {
    let output = fixture.exec_status(
        name,
        &format!("set -e; cd /tmp; case \"$(uname -m)\" in x86_64) node_arch=x64;; aarch64|arm64) node_arch=arm64;; *) echo \"unsupported guest architecture $(uname -m)\"; exit 1;; esac; \
         curl -fsSL -m 180 \"https://nodejs.org/dist/v22.11.0/node-v22.11.0-linux-$node_arch.tar.xz\" -o node.tar.xz; \
         mkdir -p /tmp/nodejs; tar -xJf node.tar.xz -C /tmp/nodejs --strip-components=1; cp /tmp/nodejs/bin/node /usr/local/bin/node; \
         export PATH=/tmp/nodejs/bin:$PATH; {INSTALL_HARNESS_PACKAGES}; \
         which codex claude; runuser -u silo -- env HOME=/home/silo codex --version; runuser -u silo -- env HOME=/home/silo claude --version"),
    );
    eprintln!("harness install:\n{output}");
    output.contains("EXIT:0")
}

/// The approval switch through Silo's own `apply_approval_with`: `auto` adds exactly
/// LCU's approval entries to the harnesses that are present (Codex: a per-server approval
/// mode; Claude Code: an allow rule) and `ask` removes exactly those. Phase one runs
/// before any harness was installed; phase two installs Codex and Claude Code from npm
/// (skipped with `SILO_LIVE_SKIP_HARNESS_INSTALL=1` when the guest has no network).
#[test]
#[ignore = "requires the v4 guest image, a published ChatGPT app and hardware virtualization"]
fn live_approval_switch_edits_only_the_installed_harnesses() {
    use crate::computer_use::{apply_approval_with, Approval};
    let _state = crate::test_support::global_state();
    let mut fixture = Fixture::new("silo-appr-", None, true);
    let name = "e2e-appr";
    let configuration = fixture.create(name);
    runtime::start_disposable_test_computer(&fixture.paths, name).unwrap();
    let (status, _) = fixture.wait_ready(name, "approval");
    assert_eq!(status["computerUse"]["approval"], "ask");
    eprintln!(
        "agents after the first setup: {}",
        status["computerUse"]["agents"]
    );
    let switch = |approval: Approval| {
        apply_approval_with(
            std::sync::Arc::new(runtime::ProcessRunner),
            &fixture.paths,
            &configuration,
            approval,
            true,
        )
        .unwrap();
        let (status, _) = fixture.wait_ready(name, &format!("approval {approval:?}"));
        assert_eq!(status["computerUse"]["approval"], approval.as_str());
        assert_eq!(status["computerUse"]["state"], "ready", "{status}");
        status
    };
    let codex = || guest_file(&fixture, name, "/home/silo/.codex/config.toml");
    let claude_settings = || guest_file(&fixture, name, "/home/silo/.claude/settings.json");
    let claude_json = || guest_file(&fixture, name, "/home/silo/.claude.json");
    let approve_line = "default_tools_approval_mode = \"approve\"";

    // Phase one: nothing installed. LCU's own installer leaves `~/.codex`, so Codex counts
    // as present; Claude Code never does.
    let ask_codex = codex();
    eprintln!("codex config (ask, no harness installed):\n{ask_codex}");
    assert!(!ask_codex.contains(approve_line));
    assert!(claude_json().is_empty() && claude_settings().is_empty());
    let status = switch(Approval::Auto);
    eprintln!("agents on auto: {}", status["computerUse"]["agents"]);
    assert!(codex().contains(approve_line), "{}", codex());
    assert!(
        claude_json().is_empty() && claude_settings().is_empty(),
        "Claude Code is not installed and must get no entries"
    );
    switch(Approval::Ask);
    assert!(!codex().contains(approve_line));
    assert!(claude_json().is_empty() && claude_settings().is_empty());

    if std::env::var("SILO_LIVE_SKIP_HARNESS_INSTALL").is_ok() {
        fixture.stop(name);
        return;
    }
    // Phase two: Codex and Claude Code installed.
    assert!(
        install_harnesses(&fixture, name),
        "could not install the harnesses"
    );
    let status = switch(Approval::Auto);
    eprintln!(
        "agents on auto with harnesses: {}",
        status["computerUse"]["agents"]
    );
    let auto_codex = codex();
    let auto_settings: Value = serde_json::from_str(&claude_settings()).unwrap();
    let auto_claude_json: Value = serde_json::from_str(&claude_json()).unwrap();
    assert!(auto_codex.contains(approve_line), "{auto_codex}");
    assert_eq!(
        auto_settings["permissions"]["allow"],
        serde_json::json!(["mcp__lcu"]),
        "{auto_settings}"
    );
    assert!(auto_claude_json["mcpServers"]["lcu"].is_object());
    switch(Approval::Ask);
    // `ask` removes exactly the approval entries and nothing else.
    assert_eq!(
        auto_codex.replace(&format!("{approve_line}\n"), ""),
        codex(),
        "ask must remove only the approval line"
    );
    let mut expected = auto_settings.clone();
    expected["permissions"]
        .as_object_mut()
        .unwrap()
        .remove("allow");
    let ask_settings: Value = serde_json::from_str(&claude_settings()).unwrap();
    assert_eq!(
        expected, ask_settings,
        "ask must remove only the allow rule"
    );
    let ask_claude_json: Value = serde_json::from_str(&claude_json()).unwrap();
    assert_eq!(auto_claude_json, ask_claude_json, "registration stays");
    eprintln!("claude settings after ask: {ask_settings}");
    fixture.stop(name);
}
