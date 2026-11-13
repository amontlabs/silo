//! Installs and inspects agents (Claude Code, Codex) in a running macOS computer.
//!
//! Like the developer tools step, the host downloads each pinned file, checks its size and
//! SHA-256, and copies it over SSH, so the computer downloads nothing itself. Claude Code is
//! already in the template, whose fingerprint this module leaves alone: installing it again
//! repairs or updates a computer whose receipt is missing or old. Silo never signs in to an
//! agent.
use super::developer_tools::{self, Asset};
use super::{guest_access, layout_and_record, MacosContext};
use serde_json::Value;
use sha2::{Digest, Sha256};
use silo_api::methods::agents::{AgentId, AgentInfo, AgentInstallStep};
use std::{
    collections::HashMap,
    fs,
    path::Path,
    sync::{Mutex, MutexGuard},
    time::{Duration, Instant},
};

const SCRIPT: &str = include_str!("../../../guest/macos/silo-agent-tools.zsh");
const LOCK: &str = include_str!("../../../guest/agent-tools-lock.json");
const STAGE: &str = "/private/tmp/silo-agent-tools";
const INSTALL_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const COPY_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const RECEIPT_READ: Duration = Duration::from_secs(8);
const RECEIPT_MAX_BYTES: usize = 8 * 1024;
/// The most an install script may print before Silo stops it.
const INSTALL_MAX_STDOUT: usize = 64 * 1024;
/// How long a read answers further lists without another login to the computer.
const RECEIPT_CACHE: Duration = Duration::from_secs(3);

type Receipts = (Option<Value>, Option<Value>);
static RECEIPTS: Mutex<Option<HashMap<String, (Instant, Receipts)>>> = Mutex::new(None);

fn receipts() -> MutexGuard<'static, Option<HashMap<String, (Instant, Receipts)>>> {
    RECEIPTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// What one install puts in the computer: the version, the staged file's lock entry and the
/// name of the one file the Codex archive holds (empty for Claude Code's installer).
struct Plan {
    version: String,
    asset: Asset,
    member: String,
}

fn plan(agent: AgentId) -> Result<Plan, String> {
    let invalid = || format!("Silo's {} lock is invalid.", agent.display_name());
    match agent {
        AgentId::ClaudeCode => {
            let (version, asset) = developer_tools::claude_code_pin()?;
            Ok(Plan {
                version,
                asset,
                member: String::new(),
            })
        }
        AgentId::Codex => {
            let codex = crate::agent_tools::codex_pin();
            let pinned = codex.assets.macos_arm64;
            let file = pinned.url.rsplit('/').next().unwrap_or_default().to_owned();
            let asset = Asset {
                file,
                url: pinned.url,
                bytes: pinned.bytes,
                sha256: pinned.sha256,
            };
            let release = format!(
                "/openai/codex/releases/download/rust-v{}/{}",
                codex.version, asset.file
            );
            if !developer_tools::validate_asset(&asset, "github.com", ".tar.gz")
                || !asset.url.ends_with(&release)
                || pinned.member != asset.file.trim_end_matches(".tar.gz")
            {
                return Err(invalid());
            }
            Ok(Plan {
                version: codex.version,
                asset,
                member: pinned.member,
            })
        }
    }
}

fn lock_digest() -> String {
    Sha256::digest(LOCK.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The script with the plan's values filled in.
fn script(agent: AgentId, plan: &Plan) -> String {
    SCRIPT
        .replace("@AGENT@", agent.as_str())
        .replace("@VERSION@", &plan.version)
        .replace("@FILE@", &plan.asset.file)
        .replace("@BYTES@", &plan.asset.bytes.to_string())
        .replace("@SHA256@", &plan.asset.sha256)
        .replace("@MEMBER@", &plan.member)
        .replace("@LOCK_SHA256@", &lock_digest())
}

/// The guest command that registers computer use with agents installed since it was set up.
/// The helper exists only in a computer that has computer use; a failure never matters, the
/// helper tries again at the next login.
const REGISTER_COMMAND: &str = "REGISTER=\"$HOME/Library/Application Support/Silo/silo-cua-register.py\"; if [ -f \"$REGISTER\" ]; then /usr/bin/python3 \"$REGISTER\" reconcile >/dev/null 2>&1 || true; fi";

/// Computer use's registration, then Silo's own MCP server's (a computer without the tools has
/// neither the program nor the endpoint file, and the second does nothing).
fn register_command() -> String {
    format!(
        "{REGISTER_COMMAND}\n{}",
        crate::guest_components::register_after_install_command(
            crate::guest_components::run::Kind::Macos
        )
    )
}

/// Prints the receipt of each agent on its own line: Claude Code, then Codex.
const READ_RECEIPTS: &str = "for a in claude-code codex; do /bin/cat \"/Library/Application Support/Silo/agents/$a.json\" 2>/dev/null | /usr/bin/tr -d '\\n'; echo; done";

/// The two receipts in what `READ_RECEIPTS` printed; a missing or unreadable one is `None`.
fn parse_receipts(stdout: &str) -> Receipts {
    let mut lines = stdout.lines();
    let mut next = || {
        lines
            .next()
            .and_then(|line| serde_json::from_str::<Value>(line.trim()).ok())
            .filter(Value::is_object)
    };
    let claude = next();
    let codex = next();
    (claude, codex)
}

/// The two receipts a running computer holds, or `Err` when the computer could not be asked.
pub(super) fn read_receipts(
    layout: &super::store::Layout,
    record: &super::store::Record,
) -> Result<Receipts, ()> {
    let output = guest_access::run_capped(
        layout,
        record,
        READ_RECEIPTS,
        None,
        RECEIPT_READ,
        Some(RECEIPT_MAX_BYTES),
    )
    .map_err(|_| ())?;
    if output.status != 0 {
        return Err(());
    }
    Ok(parse_receipts(&output.stdout))
}

/// The agents of the computer: Claude Code from its template and receipt, Codex from its
/// receipt when the computer runs and answers, and `unknown` while it is stopped or does not.
pub(super) fn list(app: &MacosContext, id: &str, running: bool) -> Result<Vec<AgentInfo>, String> {
    let (layout, record) = layout_and_record(app, id)?;
    let read: Option<Receipts> = if running {
        let cached = receipts()
            .as_ref()
            .and_then(|map| map.get(id))
            .filter(|(at, _)| at.elapsed() < RECEIPT_CACHE)
            .map(|(_, read)| read.clone());
        cached.or_else(|| {
            let read = read_receipts(&layout, &record).ok()?;
            receipts()
                .get_or_insert_with(HashMap::new)
                .insert(id.to_owned(), (Instant::now(), read.clone()));
            Some(read)
        })
    } else {
        None
    };
    let answered = read.is_some();
    let (claude, codex) = read.unwrap_or((None, None));
    let mut agents = crate::agents::from_macos(
        record.setup.developer_tools && record.developer_tools_version.is_some(),
        !record.developer_tools_stale(),
        claude.as_ref(),
        answered.then_some(codex.as_ref()),
    );
    crate::agents::mark_installing(id, &mut agents);
    Ok(agents)
}

/// Removes the archives of earlier Codex versions from the download cache.
fn prune_cache(cache: &Path, keep: &str) {
    let Ok(entries) = fs::read_dir(cache) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("codex-") && name.ends_with(".tar.gz") && name != keep {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// What to show for a failed install: the last `error: ` line the script printed, else its last
/// line, cleaned and cut short.
fn install_failure_reason(stderr: &str) -> String {
    let line = stderr
        .lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix("error: "))
        .or_else(|| stderr.lines().rev().find(|line| !line.trim().is_empty()))
        .map(crate::agents::safe_reason)
        .filter(|reason| !reason.is_empty());
    line.unwrap_or_else(|| "the installer returned an error".to_owned())
}

/// Installs `agent` in the running computer `id`: download and check on this device, copy,
/// install, then register computer use with it. `step` is told where the install is.
pub(super) fn install(
    app: &MacosContext,
    id: &str,
    agent: AgentId,
    step: &dyn Fn(AgentInstallStep, Option<String>),
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    let plan = plan(agent)?;
    let name = agent.display_name();
    let _installing = crate::agents::begin_install(id, agent)?;
    let cancelled = || cancelled() || crate::runtime::OPERATIONS.removal_queued(id);
    let cache = super::app_data(app)?.join(developer_tools::CACHE_DIR);
    step(
        AgentInstallStep::Downloading,
        Some(format!("Downloading {name}")),
    );
    let archive = {
        let _turn = developer_tools::download_turn(&cancelled)?;
        developer_tools::cached_with(&cache, &plan.asset, &cancelled, &|detail| {
            step(AgentInstallStep::Downloading, Some(detail.to_owned()))
        })?
    };
    if agent == AgentId::Codex {
        prune_cache(&cache, &plan.asset.file);
    }
    if cancelled() {
        return Err(guest_access::CANCELLED.into());
    }
    let (layout, record) = layout_and_record(app, id)?;
    step(
        AgentInstallStep::Installing,
        Some(format!("Copying {name} to the computer")),
    );
    let ready = guest_access::run_cancellable(
        &layout,
        &record,
        &format!("/bin/rm -rf {STAGE} && /bin/mkdir -p {STAGE}"),
        Duration::from_secs(60),
        &cancelled,
    )?;
    if ready.status != 0 {
        return Err("Silo could not prepare a folder in the computer.".into());
    }
    if cancelled() {
        return Err(guest_access::CANCELLED.into());
    }
    guest_access::copy_cancellable(
        &layout,
        &record,
        &archive,
        &format!("{STAGE}/{}", plan.asset.file),
        COPY_TIMEOUT,
        &cancelled,
    )?;
    step(
        AgentInstallStep::Installing,
        Some(format!("Installing {name}")),
    );
    let output = guest_access::run_cancellable_with_input_capped(
        &layout,
        &record,
        "/bin/zsh -s",
        script(agent, &plan).as_bytes(),
        INSTALL_TIMEOUT,
        &cancelled,
        Some(INSTALL_MAX_STDOUT),
    )?;
    if output.status != 0 {
        return Err(format!(
            "{name} could not be installed: {}",
            install_failure_reason(&output.stderr)
        ));
    }
    if let Some(map) = receipts().as_mut() {
        map.remove(id);
    }
    step(
        AgentInstallStep::Registering,
        Some("Adding computer use to the agent".into()),
    );
    let _ = guest_access::run_cancellable(
        &layout,
        &record,
        &register_command(),
        Duration::from_secs(120),
        &cancelled,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_codex_plan_is_the_pinned_macos_archive_from_the_official_release() {
        let plan = plan(AgentId::Codex).unwrap();
        assert_eq!(plan.version, "0.167.0");
        assert_eq!(plan.asset.file, "codex-aarch64-apple-darwin.tar.gz");
        assert_eq!(plan.member, "codex-aarch64-apple-darwin");
        assert!(plan
            .asset
            .url
            .starts_with("https://github.com/openai/codex/releases/download/rust-v0.167.0/"));
        assert!(developer_tools::validate_asset(
            &plan.asset,
            "github.com",
            ".tar.gz"
        ));
    }

    #[test]
    fn the_claude_code_plan_is_the_installer_the_template_pins() {
        let plan = plan(AgentId::ClaudeCode).unwrap();
        assert_eq!(plan.asset.file, "bootstrap.sh");
        assert_eq!(
            plan.asset.url,
            "https://downloads.claude.ai/claude-code-releases/bootstrap.sh"
        );
        // One version everywhere: the state a list reports compares with the lock of Linux.
        assert_eq!(
            plan.version,
            crate::agent_tools::expected_agent_version(AgentId::ClaudeCode)
        );
    }

    #[test]
    fn each_script_is_filled_in_without_leftover_markers() {
        for agent in AgentId::ALL {
            let plan = plan(agent).unwrap();
            let script = script(agent, &plan);
            for marker in [
                "@AGENT@",
                "@VERSION@",
                "@FILE@",
                "@BYTES@",
                "@SHA256@",
                "@MEMBER@",
                "@LOCK_SHA256@",
            ] {
                assert!(!script.contains(marker), "{agent:?}: {marker}");
            }
            assert!(script.contains(&format!("AGENT='{}'", agent.as_str())));
            assert!(script.contains(&format!("VERSION='{}'", plan.version)));
            assert!(script.contains(&format!("SHA256='{}'", plan.asset.sha256)));
            assert!(script.contains(&format!("BYTES='{}'", plan.asset.bytes)));
            assert!(script.contains(&format!("LOCK_SHA256='{}'", lock_digest())));
        }
    }

    #[test]
    fn the_script_checks_the_staged_file_and_downloads_nothing_for_codex() {
        for needle in [
            "did not match its locked size and SHA-256",
            "the Codex archive does not hold the expected file",
            "Codex version verification failed",
            "/usr/local/bin/codex",
            "agents",
        ] {
            assert!(SCRIPT.contains(needle), "{needle}");
        }
        let codex_part = &SCRIPT[SCRIPT.find("install_codex()").unwrap()
            ..SCRIPT.find("install_claude_code()").unwrap()];
        assert!(!codex_part.contains("curl"));
    }

    #[test]
    fn the_script_checks_a_root_owned_copy_and_cleans_up_on_every_exit() {
        let checked = SCRIPT.find("shasum -a 256 \"$SAFE/$FILE\"").unwrap();
        let copied = SCRIPT
            .find("install -o root -g wheel -m 0644 \"$STAGE/$FILE\" \"$SAFE/$FILE\"")
            .unwrap();
        assert!(copied < checked);
        // Nothing after the copy reads the account's staged file again.
        assert!(!SCRIPT[checked..].contains("$STAGE/$FILE"));
        assert!(SCRIPT.contains("trap cleanup EXIT"));
        assert!(SCRIPT.contains("rm -rf \"$STAGE\""));
        assert!(SCRIPT.contains("rm -rf \"$SAFE\""));
        assert!(!SCRIPT.contains("*\"$VERSION\"*"));
        assert!(!SCRIPT.contains("== \"$VERSION\"*"));
    }

    fn zsh_output(program: &str) -> Option<String> {
        if !Path::new("/bin/zsh").exists() {
            return None;
        }
        let from = SCRIPT.find("# The version in a program").unwrap();
        let to = SCRIPT.find("[[ $(/usr/bin/uname").unwrap();
        let output = std::process::Command::new("/bin/zsh")
            .arg("-c")
            .arg(format!(
                "setopt extendedglob\n{}\n{program}",
                &SCRIPT[from..to]
            ))
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    #[test]
    fn the_script_compares_versions_as_dotted_numbers_and_takes_the_version_word_exactly() {
        let Some(tokens) = zsh_output(
            "for out in 'codex-cli 0.167.0' '2.1.287 (Claude Code)' '0.167.0-beta' 'v1.2.3' ''; do print -r -- \"[$(version_token \"$out\")]\"; done",
        ) else {
            return;
        };
        assert_eq!(tokens, "[0.167.0]\n[2.1.287]\n[]\n[]\n[]\n");
        let order = zsh_output(
            "for pair in '0.167.0 0.167.0' '0.168.0 0.167.0' '0.166.9 0.167.0' '0.10.0 0.9.0' '1.0 0.167.0' '0.167 0.167.0' '08.0.0 8.0.0'; do set -- ${=pair}; not_older $1 $2 && print newer-or-same || print older; done",
        )
        .unwrap();
        assert_eq!(
            order,
            "newer-or-same\nnewer-or-same\nolder\nnewer-or-same\nnewer-or-same\nnewer-or-same\nnewer-or-same\n"
        );
    }

    #[test]
    fn the_receipts_are_read_per_line_and_a_missing_one_is_none() {
        let (claude, codex) = parse_receipts("{\"version\":\"2.1.287\"}\n\n");
        assert_eq!(claude.unwrap()["version"], "2.1.287");
        assert!(codex.is_none());
        let (claude, codex) = parse_receipts("not json\n[1]\n");
        assert!(claude.is_none() && codex.is_none());
    }

    #[test]
    fn a_failure_reason_has_no_control_characters_and_at_most_about_200_characters() {
        let long = "x".repeat(60_000);
        let stderr = format!("first\nerror: bad \u{1b}[31mred\u{7} {long}\nlast words\n");
        let reason = install_failure_reason(&stderr);
        assert!(reason.chars().count() <= 201, "{}", reason.len());
        assert!(reason.starts_with("bad [31mred "));
        assert!(!reason.chars().any(char::is_control));
        assert_eq!(
            install_failure_reason("one\nlast \u{1b}[0m line\n\n"),
            "last [0m line"
        );
        assert_eq!(
            install_failure_reason("\n\u{7}\n"),
            "the installer returned an error"
        );
    }

    #[test]
    fn the_template_fingerprint_does_not_depend_on_this_module() {
        // The template's inputs are the developer tools script and lock, which hold the
        // template's Claude Code and no Codex.
        let inputs = developer_tools::setup_inputs();
        assert!(!inputs.iter().any(|input| input.contains("codex")));
        assert!(!inputs
            .iter()
            .any(|input| input.contains("silo-agent-tools")));
    }

    #[test]
    fn the_registration_command_runs_the_computer_use_helper_only_when_it_exists() {
        assert!(REGISTER_COMMAND.starts_with("REGISTER=\"$HOME/Library/Application Support/Silo/"));
        assert!(REGISTER_COMMAND.contains("if [ -f \"$REGISTER\" ]"));
        assert!(REGISTER_COMMAND.contains(" reconcile "));
        assert!(REGISTER_COMMAND.ends_with("fi"));
    }

    #[test]
    fn silos_own_server_is_registered_beside_computer_use_after_an_install() {
        let command = register_command();
        let computer_use = command.find(" reconcile ").unwrap();
        let silo = command.find("server-add --name silo").unwrap();
        assert!(computer_use < silo);
        assert!(command.contains("[ -f /etc/silo/endpoint.json ]"));
    }
}
