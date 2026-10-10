use super::*;
use crate::runtime::CommandOutput;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

const COMPUTER_ID: &str = "00000000-0000-4000-8000-000000000001";

fn computer_configuration(built_in: bool) -> ComputerConfiguration {
    ComputerConfiguration {
        id: COMPUTER_ID.into(),
        name: "dev".into(),
        cpus: 1,
        max_cpus: 1,
        memory_gib: 1,
        max_memory_gib: 1,
        workspace_storage_gib: 1,
        runtime_storage_gib: 1,
        desktop: Some(desktop::DesktopConfiguration {
            start_with_computer: true,
            built_in,
        }),
    }
}

fn paths(directory: &tempfile::TempDir) -> RuntimePaths {
    crate::test_support::paths(directory.path())
}

/// Records commands and answers every guest `exec` with `stdout`.
struct Recorder {
    calls: StdMutex<Vec<Vec<String>>>,
    stdout: String,
}

impl Recorder {
    fn new(stdout: &str) -> Self {
        Self {
            calls: StdMutex::new(Vec::new()),
            stdout: stdout.into(),
        }
    }

    fn scripts(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(|args| args.last().unwrap().clone())
            .collect()
    }
}

impl RuntimeRunner for Recorder {
    fn run(
        &self,
        _paths: &RuntimePaths,
        args: &[String],
        _timeout: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        self.calls.lock().unwrap().push(args.to_vec());
        Ok(CommandOutput {
            stdout: self.stdout.clone(),
            stderr: String::new(),
        })
    }
}

/// A gate of this test's own, so tests never wait for (or hold) the process-wide one.
fn test_gate() -> &'static runtime::operation_gate::OperationGate {
    Box::leak(Box::new(runtime::operation_gate::OperationGate::new()))
}

fn with_published<T>(work: impl FnOnce(&Path) -> T) -> T {
    let directory = tempfile::tempdir().unwrap();
    set_test_published_dir(Some(directory.path().to_path_buf()));
    let result = work(directory.path());
    set_test_published_dir(None);
    result
}

// ---------------------------------------------------------------- mount

#[test]
fn only_built_in_computers_get_the_mount_and_it_is_read_only() {
    with_published(|dir| {
        assert_eq!(
            mount_args(&computer_configuration(false)).unwrap(),
            Vec::<String>::new()
        );
        let args = mount_args(&computer_configuration(true)).unwrap();
        assert_eq!(
            args,
            [
                "-v",
                &format!("{}:/opt/silo/chatgpt:ro,uid=0,gid=0", dir.display())
            ]
        );
    });
}

#[test]
fn the_lcu_folder_is_lent_read_only_when_silo_has_one() {
    with_published(|_| {
        let lcu = tempfile::tempdir().unwrap();
        let without = mount_args_with(&computer_configuration(true), None).unwrap();
        assert_eq!(without.len(), 2);
        let with = mount_args_with(
            &computer_configuration(true),
            Some(lcu.path().to_path_buf()),
        )
        .unwrap();
        assert_eq!(with[..2], without[..]);
        assert_eq!(
            with[2..],
            [
                "-v".to_owned(),
                format!(
                    "{}:/opt/silo/lcu:ro,uid=0,gid=0",
                    lcu.path().canonicalize().unwrap().display()
                )
            ]
        );
        // A folder that is gone is skipped, and a computer without computer use gets nothing.
        let gone = lcu.path().join("gone");
        assert_eq!(
            mount_args_with(&computer_configuration(true), Some(gone))
                .unwrap()
                .len(),
            2
        );
        assert!(mount_args_with(
            &computer_configuration(false),
            Some(lcu.path().to_path_buf())
        )
        .unwrap()
        .is_empty());
    });
}

#[test]
fn creation_applies_computer_use_in_one_temporary_boot_and_records_it() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let recorder = Recorder::new("{\"state\":\"ready\",\"apply\":{\"approval\":\"ask\",\"outcome\":\"applied\",\"reason\":null}}\n");
    finish_in_creation(&recorder, &paths, COMPUTER_ID, "dev").unwrap();
    let calls = recorder.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    // No `--no-start`: the call boots the stopped computer and the runtime stops it again.
    assert!(!calls[0].contains(&"--no-start".to_owned()));
    assert!(calls[0]
        .last()
        .unwrap()
        .contains("apply --approval ask --boot"));
    assert_eq!(
        read_policy(&paths, COMPUTER_ID)
            .last
            .map(|attempt| attempt.outcome),
        Some(Outcome::Applied)
    );
}

#[test]
fn a_failed_creation_apply_is_reported_short_and_left_for_the_first_start() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let recorder = Recorder::new("{\"state\":\"ready\",\"apply\":{\"approval\":\"ask\",\"outcome\":\"failed\",\"reason\":\"lcu-archive-unavailable\"}}\n");
    assert_eq!(
        finish_in_creation(&recorder, &paths, COMPUTER_ID, "dev"),
        Err("lcu-archive-unavailable".into())
    );
    let missing = Recorder::new("{\"state\":\"ready\",\"apply\":{\"approval\":\"ask\",\"outcome\":\"failed\",\"reason\":\"app-missing\"}}\n");
    assert!(finish_in_creation(&missing, &paths, COMPUTER_ID, "dev").is_err());
}

#[test]
fn an_existing_but_empty_shared_folder_never_blocks_the_mount() {
    with_published(|dir| {
        assert_eq!(std::fs::read_dir(dir).unwrap().count(), 0);
        assert_eq!(mount_args(&computer_configuration(true)).unwrap().len(), 2);
        // Still mounted unchanged once the app (or anything else) is published into it.
        std::fs::create_dir(dir.join("1.0-arm64")).unwrap();
        assert_eq!(mount_args(&computer_configuration(true)).unwrap().len(), 2);
    });
}

#[test]
fn a_missing_or_unusable_shared_folder_blocks_the_mount() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("file");
    std::fs::write(&file, b"x").unwrap();
    for bad in [directory.path().join("gone"), file] {
        set_test_published_dir(Some(bad));
        let error = mount_args(&computer_configuration(true))
            .unwrap_err()
            .to_string();
        assert!(error.contains("shared ChatGPT folder"), "{error}");
        assert!(mount_args(&computer_configuration(false))
            .unwrap()
            .is_empty());
    }
    set_test_published_dir(None);
}

#[test]
fn a_built_in_computer_without_a_shared_folder_is_an_error_not_a_silent_omission() {
    let _state = crate::test_support::global_state();
    reset_published_for_test();
    set_test_published_dir(None);
    let error = mount_args(&computer_configuration(true))
        .unwrap_err()
        .to_string();
    assert!(error.contains("shared ChatGPT folder"), "{error}");
    assert!(mount_args(&computer_configuration(false))
        .unwrap()
        .is_empty());
}

#[test]
fn the_mount_check_needs_a_read_only_bind_at_the_guest_path() {
    let bind = |guest: &str, readonly: bool| {
        json!({"type":"Bind","host":"/h/chatgpt/published","guest":guest,
            "options":{"readonly":readonly,"noexec":false}})
    };
    let computer = json!({"type":"Owned","guest":"/workspace",
        "storage":{"kind":"disk","capacity_mib":1024}});
    let config = |mounts: Value| json!({"mounts": mounts});
    let built_in = computer_configuration(true);
    assert!(mount_present(
        &config(json!([computer, bind("/opt/silo/chatgpt", true)])),
        &built_in
    ));
    assert!(!mount_present(&config(json!([computer])), &built_in));
    assert!(!mount_present(
        &config(json!([computer, bind("/opt/silo/chatgpt", false)])),
        &built_in
    ));
    assert!(!mount_present(
        &config(json!([computer, bind("/elsewhere", true)])),
        &built_in
    ));
    assert!(!mount_present(&json!({}), &built_in));
    // A computer without built-in computer use needs no mount.
    assert!(mount_present(
        &config(json!([computer])),
        &computer_configuration(false)
    ));
}

#[test]
fn export_drops_the_host_specific_mount_and_refuses_a_writable_one() {
    let computer = json!({"type":"Owned","guest":"/workspace"});
    let mount = json!({"type":"Bind","host":"/h/chatgpt/published","guest":"/opt/silo/chatgpt",
        "options":{"readonly":true}});
    let mut config = json!({"mounts":[computer.clone(), mount.clone()]});
    strip_mount_for_export(&mut config).unwrap();
    assert_eq!(config["mounts"], json!([computer]));
    let mut writable = json!({"mounts":[{"type":"Bind","host":"/h","guest":"/opt/silo/chatgpt",
        "options":{"readonly":false}}]});
    assert!(strip_mount_for_export(&mut writable).is_err());
    // Another bind mount is not ours to remove; the export's own checks reject it.
    let mut other = json!({"mounts":[{"type":"Bind","host":"/h","guest":"/data",
        "options":{"readonly":true}}]});
    strip_mount_for_export(&mut other).unwrap();
    assert_eq!(other["mounts"].as_array().unwrap().len(), 1);
    strip_mount_for_export(&mut json!({})).unwrap();
}

// ------------------------------------------------------------- settings

#[test]
fn approval_defaults_to_ask_and_is_kept_per_computer() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    assert_eq!(settings(&paths, COMPUTER_ID).approval, Approval::Ask);
    set_approval(&paths, COMPUTER_ID, Approval::Auto).unwrap();
    assert_eq!(settings(&paths, COMPUTER_ID).approval, Approval::Auto);
    let other = "00000000-0000-4000-8000-000000000002";
    assert_eq!(settings(&paths, other).approval, Approval::Ask);
    set_approval(&paths, COMPUTER_ID, Approval::Ask).unwrap();
    assert_eq!(settings(&paths, COMPUTER_ID).approval, Approval::Ask);
    assert!(set_approval(&paths, "../escape", Approval::Auto).is_err());
    assert_eq!(Approval::parse("auto"), Some(Approval::Auto));
    assert_eq!(Approval::parse("yes"), None);
}

#[test]
fn a_damaged_settings_file_means_ask() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    set_approval(&paths, COMPUTER_ID, Approval::Auto).unwrap();
    fs::write(policy_path(&paths, COMPUTER_ID).unwrap(), b"{not json").unwrap();
    assert_eq!(settings(&paths, COMPUTER_ID).approval, Approval::Ask);
}

#[test]
fn oversized_computer_use_policy_remains_unreadable_and_untouched() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let path = policy_path(&paths, COMPUTER_ID).unwrap();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut bytes = br#"{"approval":"auto"}"#.to_vec();
    bytes.resize(1024 * 1024, b' ');
    fs::write(&path, &bytes).unwrap();
    assert_eq!(
        read_policy_checked(&paths, COMPUTER_ID).unwrap().approval,
        Approval::Auto
    );
    bytes.push(b' ');
    fs::write(&path, &bytes).unwrap();
    let stored = settings(&paths, COMPUTER_ID);
    assert!(stored.unreadable);
    assert_eq!(stored.approval, Approval::Ask);
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn a_fifo_computer_use_record_is_refused_without_waiting_for_a_writer() {
    use std::os::unix::fs::OpenOptionsExt;
    let directory = tempfile::tempdir().unwrap();
    let record = directory.path().join("policy.json");
    let name = std::ffi::CString::new(record.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    let (send, receive) = std::sync::mpsc::channel();
    let to_read = record.clone();
    let reader = std::thread::spawn(move || {
        send.send(read_settings_bytes(&to_read)).unwrap();
    });
    let result = receive.recv_timeout(Duration::from_secs(1));
    if result.is_err() {
        // Release a blocked read before failing so the fixture leaves no reader behind.
        drop(
            fs::OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&record)
                .unwrap(),
        );
    }
    reader.join().unwrap();
    assert!(result
        .expect("settings reads must not wait for a FIFO writer")
        .is_err());
}

#[test]
fn oversized_computer_use_observation_is_ignored_without_changing_the_policy() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let policy = policy_path(&paths, COMPUTER_ID).unwrap();
    let observed = observed_path(&paths, COMPUTER_ID).unwrap();
    fs::create_dir_all(policy.parent().unwrap()).unwrap();
    fs::write(&policy, br#"{"approval":"auto"}"#).unwrap();
    let mut bytes = br#"{"state":"ready"}"#.to_vec();
    bytes.resize(1024 * 1024, b' ');
    fs::write(&observed, &bytes).unwrap();
    assert_eq!(settings(&paths, COMPUTER_ID).known.unwrap().state, "ready");
    bytes.push(b' ');
    fs::write(&observed, &bytes).unwrap();
    let stored = settings(&paths, COMPUTER_ID);
    assert!(stored.known.is_none());
    assert!(!stored.unreadable);
    assert_eq!(stored.approval, Approval::Auto);
    assert_eq!(fs::read(&observed).unwrap(), bytes);
}

#[test]
fn unfamiliar_saved_attempt_outcome_preserves_the_approval_choice() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    fs::create_dir_all(directory_of(&paths)).unwrap();
    let saved = br#"{"approval":"auto","applied":"auto","last":{"mode":"auto","outcome":"future-outcome","at":1790000000}}"#;
    fs::write(policy_path(&paths, COMPUTER_ID).unwrap(), saved).unwrap();

    let stored = settings(&paths, COMPUTER_ID);
    assert!(!stored.unreadable);
    assert_eq!(stored.approval, Approval::Auto);
    assert_eq!(stored.applied, Some(Approval::Auto));
    assert_eq!(stored.last.as_ref().unwrap().outcome, Outcome::Failed);
    let policy = read_policy_checked(&paths, COMPUTER_ID).unwrap();
    assert!(policy.needs_apply());
    write_atomic(&paths, policy_path(&paths, COMPUTER_ID), &policy).unwrap();
    assert_eq!(settings(&paths, COMPUTER_ID), stored);
    assert!(Outcome::parse("future-outcome").is_none());
}

#[test]
fn unfamiliar_saved_approval_modes_remain_unreadable() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    fs::create_dir_all(directory_of(&paths)).unwrap();
    for saved in [
        br#"{"approval":"future-mode"}"#.as_slice(),
        br#"{"approval":"auto","applied":"future-mode"}"#,
        br#"{"approval":"auto","unfinished":"future-mode"}"#,
        br#"{"approval":"auto","last":{"mode":"future-mode","outcome":"applied","at":1}}"#,
        br#"{"approval":"auto","last":{"mode":"auto","outcome":null,"at":1}}"#,
    ] {
        fs::write(policy_path(&paths, COMPUTER_ID).unwrap(), saved).unwrap();
        assert!(settings(&paths, COMPUTER_ID).unreadable);
        assert_eq!(
            fs::read(policy_path(&paths, COMPUTER_ID).unwrap()).unwrap(),
            saved
        );
    }
}

#[test]
fn a_policy_of_an_older_version_keeps_its_choice_and_applies_again() {
    // Older files carry a revision and a generation; both are ignored, the choice stays,
    // and with no attempt on record the next boot applies it.
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    fs::create_dir_all(directory_of(&paths)).unwrap();
    fs::write(
        policy_path(&paths, COMPUTER_ID).unwrap(),
        br#"{"approval":"auto","revision":1790000000000,"generation":"22222222-2222-4222-8222-222222222222"}"#,
    )
    .unwrap();
    let stored = settings(&paths, COMPUTER_ID);
    assert_eq!(stored.approval, Approval::Auto);
    assert!(!stored.unreadable);
    assert_eq!((stored.applied, stored.last), (None, None));
    assert!(read_policy(&paths, COMPUTER_ID).needs_apply());
}

fn directory_of(paths: &RuntimePaths) -> PathBuf {
    directory(paths)
}

#[test]
fn forks_inherit_only_the_approval_and_deleted_computers_are_forgotten() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let child = "00000000-0000-4000-8000-000000000003";
    set_approval(&paths, COMPUTER_ID, Approval::Auto).unwrap();
    record_attempt(
        &paths,
        COMPUTER_ID,
        attempt(Approval::Auto, Outcome::Applied, None),
    );
    remember(
        &paths,
        COMPUTER_ID,
        Known {
            state: "ready".into(),
            app_version: Some("1".into()),
            ..Known::default()
        },
    );
    inherit_settings(&paths, COMPUTER_ID, child).unwrap();
    let inherited = settings(&paths, child);
    assert_eq!(inherited.approval, Approval::Auto);
    // The fork's disk carries the source's configuration: nothing is known to be applied,
    // so its first boot applies the inherited mode.
    assert_eq!((inherited.applied, inherited.last), (None, None));
    assert_eq!(inherited.known, None);
    assert!(read_policy(&paths, child).needs_apply());
    forget(&paths, COMPUTER_ID).unwrap();
    assert_eq!(settings(&paths, COMPUTER_ID), Settings::default());
    assert!(policy_path(&paths, child).unwrap().exists());
}

fn attempt(mode: Approval, outcome: Outcome, reason: Option<&str>) -> Attempt {
    Attempt {
        mode,
        outcome,
        at: 1_790_000_000,
        reason: reason.map(str::to_owned),
    }
}

// -------------------------------------------------------- pinned pair

#[test]
fn the_pinned_pair_comes_from_the_two_locks_and_agrees() {
    let lcu: Value = serde_json::from_str(LCU_LOCK).unwrap();
    assert_eq!(lcu["version"], "0.11.0");
    assert_eq!(
        lcu["assets"]["arm64"]["sha256"],
        "bfb91127e103065e47088545c4d71dcb00714eec05fcf72ff30913212c485dc8"
    );
    assert_eq!(
        lcu["assets"]["amd64"]["sha256"],
        "12919c3bd94f1d74874e8a4da4e5d7c713138079b613df05e5f81a1e03e6a62c"
    );
    for (arch, archive) in [
        (DebArch::Arm64, "lcu-0.11.0-linux-arm64.tar.gz"),
        (DebArch::Amd64, "lcu-0.11.0-linux-x64.tar.gz"),
    ] {
        let pair = pinned(arch).unwrap();
        assert_eq!(pair["lcu"]["version"], "0.11.0");
        assert_eq!(pair["lcu"]["archive"], archive);
        assert!(pair["lcu"]["url"].as_str().unwrap().ends_with(archive));
        let app = chatgpt_app::Lock::bundled().unwrap();
        assert_eq!(pair["app"]["dir"], app.directory_name(arch));
        assert_eq!(pair["app"]["version"], app.version);
    }
}

#[test]
fn the_pushed_helper_registers_every_agent_and_agents_installed_later() {
    // Each boot and app-ready runs `apply --boot`, which reconciles even when nothing else
    // changed; the same pushed helper carries the commands the watcher and login hook use.
    for part in [
        "'--agent', 'all', '--allow-missing'",
        "'--agent', 'auto'",
        "'--reconcile'",
        "commands.add_parser('reconcile')",
        "commands.add_parser('watch')",
        "/etc/profile.d/silo-computer-use.sh",
    ] {
        assert!(HELPER.contains(part), "{part}");
    }
    let script = guest_script(
        &pinned(DebArch::Arm64).unwrap(),
        &apply_command(Approval::Ask, false, true),
    );
    assert!(script.contains("silo-computer-use apply --approval ask --boot"));
}

#[test]
fn the_guest_script_installs_the_helper_and_pair_before_running_the_command() {
    let pair = pinned(DebArch::Arm64).unwrap();
    let script = guest_script(&pair, &apply_command(Approval::Auto, true, false));
    let helper_at = script.find("SILO_CU_HELPER_EOF").unwrap();
    let pinned_at = script.find("SILO_CU_PINNED_EOF").unwrap();
    let run_at = script
        .find("/usr/local/libexec/silo-computer-use apply --approval auto --force")
        .unwrap();
    assert!(helper_at < pinned_at && pinned_at < run_at);
    assert!(script.contains("#!/usr/bin/python3"));
    assert!(script.contains(&pair.to_string()));
    // Quoted delimiters keep the helper and the pair literal.
    assert!(script.contains("<<'SILO_CU_HELPER_EOF'"));
    assert!(script.contains("<<'SILO_CU_PINNED_EOF'"));
    assert_eq!(
        apply_command(Approval::Ask, false, true),
        "/usr/local/libexec/silo-computer-use apply --approval ask --boot"
    );
    // The removed ordering machinery is never sent.
    for command in [
        apply_command(Approval::Auto, true, true),
        apply_command(Approval::Ask, false, false),
    ] {
        assert!(!command.contains("--revision") && !command.contains("--generation"));
        assert!(!command.contains("sync") && !command.contains("setsid"));
    }
}

// ----------------------------------------------------- guest commands

/// Every fixture gets a fresh computer id because pending applies are shared by the process.
fn computer() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[test]
fn independent_computer_fixtures_do_not_share_pending_approval_state() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let other = computer();
    let id = computer();
    set_approval(&paths, &id, Approval::Ask).unwrap();
    record_attempt(&paths, &id, attempt(Approval::Ask, Outcome::Applied, None));
    let _pending = Pending::begin(&other);
    let state = desktop_state(&paths, &computer_of(&id, true), false, None).unwrap();
    assert_eq!(state["approvalApply"], "applied");
}

fn computer_of(id: &str, built_in: bool) -> ComputerConfiguration {
    ComputerConfiguration {
        id: id.into(),
        ..computer_configuration(built_in)
    }
}

fn write_computers_of(paths: &RuntimePaths, id: &str) {
    let request = runtime::ComputerConfigurationRequest {
        schema_version: 1,
        computers: vec![computer_of(id, true)],
    };
    runtime::write_metadata(&paths.metadata, &request).unwrap();
}

fn write_computers(paths: &RuntimePaths, built_in: bool) {
    let request = runtime::ComputerConfigurationRequest {
        schema_version: 1,
        computers: vec![computer_configuration(built_in)],
    };
    runtime::write_metadata(&paths.metadata, &request).unwrap();
}

/// How the simulated helper answers one `apply`.
#[derive(Clone, Debug)]
enum Reply {
    Report(&'static str, Option<&'static str>),
    TimedOut,
    Unreachable,
    /// The runtime ran `msb exec` and it failed with this explanation.
    Failed(&'static str),
}

/// A guest that answers like the helper: `inspect` as a running labelled computer, `status`,
/// and `apply --approval <mode>` with the next planned reply (applied by default). It
/// records every run and what the agents' configuration holds, and a run can be held until
/// the test releases it.
struct Guest {
    id: String,
    /// Inspections answered so far, and after how many the computer becomes another instance.
    inspects: StdMutex<(usize, Option<usize>)>,
    plan: StdMutex<std::collections::VecDeque<Reply>>,
    /// `(mode, force, boot, timeout)` of every run, in the order they reached the guest.
    runs: StdMutex<Vec<(String, bool, bool, Duration)>>,
    /// The mode `lcu setup` last left in the agents' configuration.
    configured: StdMutex<Option<String>>,
    /// Set while the next run must stall; it signals `entered` first.
    stall: StdMutex<Option<(std::sync::mpsc::Sender<()>, Arc<std::sync::Barrier>)>>,
}

impl Guest {
    fn new(id: &str) -> Arc<Self> {
        Arc::new(Self {
            id: id.into(),
            inspects: StdMutex::new((0, None)),
            plan: StdMutex::new(std::collections::VecDeque::new()),
            runs: StdMutex::new(Vec::new()),
            configured: StdMutex::new(None),
            stall: StdMutex::new(None),
        })
    }

    fn plan(&self, replies: impl IntoIterator<Item = Reply>) {
        self.plan.lock().unwrap().extend(replies);
    }

    fn modes(&self) -> Vec<String> {
        self.runs
            .lock()
            .unwrap()
            .iter()
            .map(|run| run.0.clone())
            .collect()
    }

    fn configured(&self) -> Option<String> {
        self.configured.lock().unwrap().clone()
    }
}

impl RuntimeRunner for Guest {
    fn run(
        &self,
        _paths: &RuntimePaths,
        args: &[String],
        timeout: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        let output = |stdout: String| {
            Ok(CommandOutput {
                stdout,
                stderr: String::new(),
            })
        };
        if args.first().is_some_and(|a| a == "inspect") {
            let mut inspects = self.inspects.lock().unwrap();
            inspects.0 += 1;
            let replaced = inspects.1.is_some_and(|after| inspects.0 > after);
            return output(
                json!({"name":"dev","status":"Running",
                    "config":{"labels":{"silo.machine-id":self.id}},
                    "runtime_instance_id": if replaced { "two" } else { "one" }})
                .to_string(),
            );
        }
        let script = args.last().unwrap();
        if script == STATUS_COMMAND {
            return output(json!({"state":"ready"}).to_string());
        }
        // The helper's own source precedes the command; the command is the last line.
        let words: Vec<&str> = script.lines().last().unwrap().split_whitespace().collect();
        let at = words.iter().position(|word| *word == "--approval").unwrap();
        let mode = words[at + 1].to_owned();
        let stall = self.stall.lock().unwrap().take();
        if let Some((entered, release)) = stall {
            entered.send(()).unwrap();
            release.wait();
        }
        self.runs.lock().unwrap().push((
            mode.clone(),
            words.contains(&"--force"),
            words.contains(&"--boot"),
            timeout,
        ));
        let reply = self
            .plan
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Reply::Report("applied", None));
        match reply {
            Reply::TimedOut => Err(RuntimeError::TimedOut {
                operation: "exec".into(),
            }),
            Reply::Unreachable => Err(RuntimeError::Unavailable("guest unreachable".into())),
            Reply::Failed(detail) => Err(RuntimeError::Failed {
                operation: "exec".into(),
                exit_code: Some(1),
                detail: detail.into(),
            }),
            Reply::Report(outcome, reason) => {
                // `lcu setup` changed the configuration of the agents it reached.
                if outcome != "failed" {
                    *self.configured.lock().unwrap() = Some(mode.clone());
                }
                output(
                    json!({"state":"ready","reason":null,
                        "apply":{"approval":mode,"outcome":outcome,"reason":reason}})
                    .to_string(),
                )
            }
        }
    }
}

fn boot_of(
    gate: &'static runtime::operation_gate::OperationGate,
    guest: &Arc<Guest>,
    paths: &RuntimePaths,
) -> Option<std::thread::JoinHandle<()>> {
    apply_with(gate, guest.clone(), paths, "dev", Trigger::Boot)
}

fn answer_of(paths: &RuntimePaths, id: &str, running: bool) -> Value {
    desktop_state(paths, &computer_of(id, true), running, None).unwrap()
}

#[test]
fn after_boot_runs_the_helper_to_completion_with_the_computers_chosen_mode() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    set_approval(&paths, &id, Approval::Auto).unwrap();
    let guest = Guest::new(&id);
    boot_of(test_gate(), &guest, &paths)
        .unwrap()
        .join()
        .unwrap();
    let runs = guest.runs.lock().unwrap().clone();
    assert_eq!(runs.len(), 1);
    assert_eq!(
        (runs[0].0.as_str(), runs[0].1, runs[0].2),
        ("auto", false, true)
    );
    // Synchronous and bounded: never detached, never unbounded.
    assert_eq!(runs[0].3, APPLY_TIMEOUT + APPLY_GRACE);
    assert!(APPLY_TIMEOUT <= Duration::from_secs(20 * 60));
    let stored = settings(&paths, &id);
    assert_eq!(stored.applied, Some(Approval::Auto));
    let last = stored.last.unwrap();
    assert_eq!(
        (last.mode, last.outcome, last.reason),
        (Approval::Auto, Outcome::Applied, None)
    );
    assert!(last.at > 0);
    let state = answer_of(&paths, &id, true);
    assert_eq!(state["approvalApply"], "applied");
    assert_eq!(state["appliedApproval"], "auto");
}

#[test]
fn a_computer_booted_by_its_pending_restore_still_gets_its_apply() {
    // Imports and forks boot inside the restore, while Silo still records them as pending.
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    write_computers(&paths, true);
    runtime::checkpoints::import_pending_restore(
        &paths,
        COMPUTER_ID,
        "silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9",
        "silo-backup-0-330418-1790360984903",
    )
    .unwrap();
    assert!(runtime::is_pending_restore(&paths, "dev"));
    let guest = Guest::new(COMPUTER_ID);
    boot_of(test_gate(), &guest, &paths)
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(guest.modes(), ["ask"]);
}

#[test]
fn after_boot_does_nothing_for_computers_without_built_in_computer_use() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    write_computers(&paths, false);
    let guest = Guest::new(COMPUTER_ID);
    assert!(boot_of(test_gate(), &guest, &paths).is_none());
    assert!(apply_with(test_gate(), guest.clone(), &paths, "unknown", Trigger::Boot).is_none());
    assert!(guest.runs.lock().unwrap().is_empty());
    assert_eq!(guest.inspects.lock().unwrap().0, 0);
}

#[test]
fn a_boot_that_cannot_start_computer_use_still_succeeds() {
    struct Failing;
    impl RuntimeRunner for Failing {
        fn run(
            &self,
            _: &RuntimePaths,
            _: &[String],
            _: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            Err(RuntimeError::Unavailable("guest unreachable".into()))
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    write_computers(&paths, true);
    // A runtime that cannot even inspect the computer means nothing is run.
    if let Some(handle) = apply_with(test_gate(), Arc::new(Failing), &paths, "dev", Trigger::Boot) {
        handle.join().unwrap();
    }
}

#[test]
fn a_stalled_guest_never_delays_the_boot_that_scheduled_the_apply() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    let guest = Guest::new(&id);
    let (entered, entered_receiver) = std::sync::mpsc::channel();
    let release = Arc::new(std::sync::Barrier::new(2));
    *guest.stall.lock().unwrap() = Some((entered, release.clone()));
    let started = std::time::Instant::now();
    let handle = boot_of(test_gate(), &guest, &paths).unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
    entered_receiver
        .recv_timeout(Duration::from_secs(10))
        .expect("the helper reached the guest");
    // Still running in the guest, yet the caller already returned, and the state says so.
    assert!(!handle.is_finished());
    assert_eq!(answer_of(&paths, &id, true)["approvalApply"], "pending");
    release.wait();
    handle.join().unwrap();
    assert_eq!(answer_of(&paths, &id, true)["approvalApply"], "applied");
}

// --------------------------------------------------------- serialization

#[test]
fn queued_applies_never_write_an_older_choice_over_a_newer_one() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    set_approval(&paths, &id, Approval::Auto).unwrap();
    let guest = Guest::new(&id);
    let gate = test_gate();
    let (entered, entered_receiver) = std::sync::mpsc::channel();
    let release = Arc::new(std::sync::Barrier::new(2));
    *guest.stall.lock().unwrap() = Some((entered, release.clone()));
    // The boot's apply carries `auto` and is held inside the guest.
    let first = boot_of(gate, &guest, &paths).unwrap();
    entered_receiver
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    // The user then flips the switch three times; each change queues its own apply.
    let configuration = computer_of(&id, true);
    let mut queued = Vec::new();
    for approval in [Approval::Ask, Approval::Auto, Approval::Ask] {
        let handle = apply_approval_in(gate, guest.clone(), &paths, &configuration, approval, true)
            .unwrap()
            .expect("a change that needs applying starts one");
        queued.push(handle);
        assert_eq!(answer_of(&paths, &id, true)["approvalApply"], "pending");
    }
    assert_eq!(settings(&paths, &id).approval, Approval::Ask);
    release.wait();
    first.join().unwrap();
    for handle in queued {
        handle.join().unwrap();
    }
    // The held run finished with `auto`, then exactly one more run applied the *current*
    // choice; the other queued applies found nothing left to do. Never `ask` then `auto`.
    assert_eq!(guest.modes(), ["auto", "ask"]);
    assert_eq!(guest.configured().as_deref(), Some("ask"));
    let stored = settings(&paths, &id);
    assert_eq!(
        (stored.approval, stored.applied),
        (Approval::Ask, Some(Approval::Ask))
    );
    let state = answer_of(&paths, &id, true);
    assert_eq!(
        (state["approvalApply"].as_str(), state["approval"].as_str()),
        (Some("applied"), Some("ask"))
    );
}

#[test]
fn changing_the_approval_of_a_running_computer_applies_it_in_the_background() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    let guest = Guest::new(&id);
    let gate = test_gate();
    let configuration = computer_of(&id, true);
    // The choice is stored and the state is pending before anything ran.
    let held = gate.computer(&id, "dev", "Another operation").unwrap();
    let handle = apply_approval_in(
        gate,
        guest.clone(),
        &paths,
        &configuration,
        Approval::Auto,
        true,
    )
    .unwrap()
    .unwrap();
    let state = answer_of(&paths, &id, true);
    assert_eq!(
        (state["approval"].as_str(), state["approvalApply"].as_str()),
        (Some("auto"), Some("pending"))
    );
    assert_eq!(state["appliedApproval"], "unknown");
    assert!(guest.runs.lock().unwrap().is_empty());
    drop(held);
    handle.join().unwrap();
    assert_eq!(guest.modes(), ["auto"]);
    let state = answer_of(&paths, &id, true);
    assert_eq!(
        (
            state["approvalApply"].as_str(),
            state["appliedApproval"].as_str()
        ),
        (Some("applied"), Some("auto"))
    );
    // Choosing what is already applied starts nothing.
    assert!(apply_approval_in(
        gate,
        guest.clone(),
        &paths,
        &configuration,
        Approval::Auto,
        true
    )
    .unwrap()
    .is_none());
    assert_eq!(guest.runs.lock().unwrap().len(), 1);
}

#[test]
fn changing_the_approval_of_a_stopped_computer_only_saves_it_for_the_next_boot() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    let guest = Guest::new(&id);
    assert!(apply_approval_in(
        test_gate(),
        guest.clone(),
        &paths,
        &computer_of(&id, true),
        Approval::Auto,
        false
    )
    .unwrap()
    .is_none());
    assert!(guest.runs.lock().unwrap().is_empty());
    assert_eq!(guest.inspects.lock().unwrap().0, 0);
    let state = answer_of(&paths, &id, false);
    assert_eq!(
        (state["approval"].as_str(), state["approvalApply"].as_str()),
        (Some("auto"), Some("pending"))
    );
    assert_eq!(state["appliedApproval"], "unknown");
}

// ------------------------------------------------- cancel, timeout, results

/// A guest whose helper hangs like an unresponsive one: it returns only once the operation
/// is cancelled (what the runtime does to its child) or after `give_up`.
struct Hanging {
    inner: Arc<Guest>,
    entered: StdMutex<Option<std::sync::mpsc::Sender<()>>>,
    give_up: Duration,
}

impl RuntimeRunner for Hanging {
    fn run(
        &self,
        paths: &RuntimePaths,
        args: &[String],
        timeout: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        if args
            .last()
            .is_some_and(|script| script.contains(" apply --approval"))
        {
            if let Some(entered) = self.entered.lock().unwrap().take() {
                entered.send(()).unwrap();
            }
            let deadline = std::time::Instant::now() + self.give_up;
            while std::time::Instant::now() < deadline {
                if runtime::operation_gate::cancel_requested() {
                    return Err(RuntimeError::Cancelled {
                        operation: "exec".into(),
                    });
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            return Err(RuntimeError::Unavailable("guest never answered".into()));
        }
        self.inner.run(paths, args, timeout)
    }
}

fn hanging(id: &str) -> (Arc<Hanging>, std::sync::mpsc::Receiver<()>) {
    let (entered, receiver) = std::sync::mpsc::channel();
    (
        Arc::new(Hanging {
            inner: Guest::new(id),
            entered: StdMutex::new(Some(entered)),
            give_up: Duration::from_secs(60),
        }),
        receiver,
    )
}

#[test]
fn a_stop_of_the_computer_cancels_a_running_apply_promptly_and_the_result_is_kept() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    set_approval(&paths, &id, Approval::Auto).unwrap();
    let (guest, entered) = hanging(&id);
    let gate = test_gate();
    let handle = apply_with(gate, guest, &paths, "dev", Trigger::Boot).unwrap();
    entered
        .recv_timeout(Duration::from_secs(10))
        .expect("the helper reached the guest");
    // Another computer's stop does not touch it.
    drop(
        gate.kind(runtime::operation_gate::OperationKind::Lifecycle)
            .computer(&computer(), "other", "Stopping other")
            .unwrap(),
    );
    std::thread::sleep(Duration::from_millis(200));
    assert!(!handle.is_finished());
    // This computer's stop queues behind the apply's turn and must get it long before the guest
    // answers.
    let started = std::time::Instant::now();
    let stop = gate
        .kind(runtime::operation_gate::OperationKind::Lifecycle)
        .computer(&id, "dev", "Stopping dev")
        .unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "stop waited {:?}",
        started.elapsed()
    );
    drop(stop);
    handle.join().unwrap();
    let stored = settings(&paths, &id);
    let last = stored.last.expect("the interruption is kept");
    assert_eq!(
        (last.mode, last.outcome, last.reason.as_deref()),
        (Approval::Auto, Outcome::Failed, Some("cancelled"))
    );
    assert_eq!(stored.applied, None);
    let state = answer_of(&paths, &id, false);
    assert_eq!(state["approvalApply"], "failed");
    assert!(state["approvalApplyReason"]
        .as_str()
        .unwrap()
        .contains("interrupted"));
    // The next boot tries again.
    assert!(read_policy(&paths, &id).needs_apply());
    let guest = Guest::new(&id);
    boot_of(test_gate(), &guest, &paths)
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(settings(&paths, &id).applied, Some(Approval::Auto));
}

#[test]
fn quit_cancels_a_running_apply_whatever_computer_it_names() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    let (guest, entered) = hanging(&id);
    let gate = test_gate();
    let handle = apply_with(gate, guest, &paths, "dev", Trigger::Boot).unwrap();
    entered
        .recv_timeout(Duration::from_secs(10))
        .expect("the helper reached the guest");
    // Quit is device-wide (no computer id) and waits for every running operation.
    let started = std::time::Instant::now();
    let quit = gate
        .kind(runtime::operation_gate::OperationKind::Shutdown)
        .device("Quitting")
        .unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "quit waited {:?}",
        started.elapsed()
    );
    drop(quit);
    handle.join().unwrap();
    let last = settings(&paths, &id).last.unwrap();
    assert_eq!(
        (last.outcome, last.reason.as_deref()),
        (Outcome::Failed, Some("cancelled"))
    );
}

#[test]
fn a_device_wide_operation_that_is_not_a_shutdown_does_not_cancel_an_apply() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    let (guest, entered) = hanging(&id);
    let gate = test_gate();
    let handle = apply_with(gate, guest, &paths, "dev", Trigger::Boot).unwrap();
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    let waiting = std::thread::spawn(move || {
        drop(gate.device("Creating a computer").unwrap());
    });
    std::thread::sleep(Duration::from_millis(300));
    assert!(!handle.is_finished(), "unrelated work waits for the apply");
    // End it through the computer's stop.
    drop(
        gate.kind(runtime::operation_gate::OperationKind::Lifecycle)
            .computer(&id, "dev", "Stopping dev")
            .unwrap(),
    );
    handle.join().unwrap();
    waiting.join().unwrap();
}

#[test]
fn an_apply_that_times_out_is_recorded_as_failed_and_retried_at_the_next_boot() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    set_approval(&paths, &id, Approval::Auto).unwrap();
    let guest = Guest::new(&id);
    guest.plan([Reply::TimedOut]);
    boot_of(test_gate(), &guest, &paths)
        .unwrap()
        .join()
        .unwrap();
    let stored = settings(&paths, &id);
    let last = stored.last.unwrap();
    assert_eq!(
        (last.mode, last.outcome, last.reason.as_deref()),
        (Approval::Auto, Outcome::Failed, Some("timed-out"))
    );
    assert_eq!(stored.applied, None);
    let state = answer_of(&paths, &id, true);
    assert_eq!(state["approvalApply"], "failed");
    assert!(state["approvalApplyReason"]
        .as_str()
        .unwrap()
        .contains("too long"));
    // The guest was asked within the bound the host waits for, and its own `--timeout`.
    assert!(APPLY_TIMEOUT + APPLY_GRACE < Duration::from_secs(30 * 60));
    boot_of(test_gate(), &guest, &paths)
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(answer_of(&paths, &id, true)["approvalApply"], "applied");
}

#[test]
fn the_guest_helper_is_started_with_a_bounded_timeout_argument() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let recorder = Recorder::new("{\"state\":\"ready\",\"apply\":{\"approval\":\"ask\",\"outcome\":\"applied\",\"reason\":null}}\n");
    let (_, report) = run_helper(&recorder, &paths, "dev", Approval::Ask, false, false).unwrap();
    assert_eq!(report, Report::Done(Outcome::Applied, None));
    let calls = recorder.calls.lock().unwrap().clone();
    let timeout = calls[0].iter().position(|arg| arg == "--timeout").unwrap();
    assert_eq!(
        calls[0][timeout + 1],
        format!("{}s", APPLY_TIMEOUT.as_secs())
    );
    assert!(calls[0].contains(&"--no-start".to_owned()));
}

#[test]
fn failed_partial_and_unreachable_results_are_kept_and_reported() {
    let gate = test_gate();
    for (reply, expected, reason_part) in [
        (
            Reply::Report("failed", Some("setup-failed")),
            "failed",
            "approval settings",
        ),
        (
            Reply::Report("partial", Some("setup-partial")),
            "partial",
            "Some agents",
        ),
        (Reply::Unreachable, "failed", "could not reach"),
        (
            Reply::Report("failed", Some("mount-writable")),
            "failed",
            "writable",
        ),
    ] {
        let id = computer();
        let directory = tempfile::tempdir().unwrap();
        let paths = self::paths(&directory);
        write_computers_of(&paths, &id);
        let guest = Guest::new(&id);
        // Applied ask earlier; the user then chose auto and the apply ended as planned.
        boot_of(gate, &guest, &paths).unwrap().join().unwrap();
        guest.plan([reply]);
        apply_approval_in(
            gate,
            guest.clone(),
            &paths,
            &computer_of(&id, true),
            Approval::Auto,
            true,
        )
        .unwrap()
        .unwrap()
        .join()
        .unwrap();
        let state = answer_of(&paths, &id, true);
        assert_eq!(state["approval"], "auto");
        assert_eq!(state["approvalApply"], expected, "{state}");
        assert!(
            state["approvalApplyReason"]
                .as_str()
                .unwrap()
                .contains(reason_part),
            "{state}"
        );
        // The previous successful mode stays visible and nothing is assumed rolled back.
        assert_eq!(state["appliedApproval"], "ask");
        let last = settings(&paths, &id).last.unwrap();
        assert_eq!(last.mode, Approval::Auto);
        assert!(
            read_policy(&paths, &id).needs_apply(),
            "it is retried at the next boot"
        );
    }
}

#[test]
fn a_partial_result_is_never_taken_for_applied_even_after_the_choice_changes_back() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    let guest = Guest::new(&id);
    let gate = test_gate();
    boot_of(gate, &guest, &paths).unwrap().join().unwrap();
    guest.plan([Reply::Report("partial", Some("setup-partial"))]);
    let configuration = computer_of(&id, true);
    apply_approval_in(
        gate,
        guest.clone(),
        &paths,
        &configuration,
        Approval::Auto,
        true,
    )
    .unwrap()
    .unwrap()
    .join()
    .unwrap();
    // Back to the mode that was applied before the partial run: the last attempt (auto,
    // partial) does not make `ask` current, so it is applied again.
    let handle = apply_approval_in(
        gate,
        guest.clone(),
        &paths,
        &configuration,
        Approval::Ask,
        true,
    )
    .unwrap();
    handle.expect("ask must be applied again").join().unwrap();
    assert_eq!(guest.modes(), ["ask", "auto", "ask"]);
    assert_eq!(answer_of(&paths, &id, true)["approvalApply"], "applied");
}

#[test]
fn an_app_that_is_not_there_yet_is_not_a_result_and_the_apply_stays_pending() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    let guest = Guest::new(&id);
    guest.plan([Reply::Report("failed", Some("app-missing"))]);
    boot_of(test_gate(), &guest, &paths)
        .unwrap()
        .join()
        .unwrap();
    assert!(settings(&paths, &id).last.is_none());
    assert_eq!(answer_of(&paths, &id, true)["approvalApply"], "pending");
    // When the app becomes ready the apply runs for real.
    boot_of(test_gate(), &guest, &paths)
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(answer_of(&paths, &id, true)["approvalApply"], "applied");
}

#[test]
fn a_report_the_host_cannot_read_is_a_failed_attempt_not_a_crash() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let broken = Recorder::new("not json");
    let run = run_helper(&broken, &paths, "dev", Approval::Ask, false, false);
    assert!(matches!(run, Err(RuntimeError::Malformed(_))));
    assert_eq!(
        attempt_of(Approval::Ask, &run).unwrap().reason.as_deref(),
        Some("invalid-report")
    );
    let missing = Recorder::new("{\"state\":\"ready\"}\n");
    assert!(run_helper(&missing, &paths, "dev", Approval::Ask, false, false).is_err());
    let odd = Recorder::new("{\"apply\":{\"outcome\":\"maybe\"}}\n");
    assert!(run_helper(&odd, &paths, "dev", Approval::Ask, false, false).is_err());
}

#[test]
fn setup_reruns_with_force_applies_the_chosen_mode_and_returns_the_guest_status() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    set_approval(&paths, &id, Approval::Auto).unwrap();
    let recorder = Recorder::new("noise\n{\"state\":\"ready\",\"apply\":{\"approval\":\"auto\",\"outcome\":\"applied\",\"reason\":null}}\n");
    let idle = || Arc::new(std::sync::atomic::AtomicBool::new(false));
    let status = setup_with(
        test_gate(),
        &recorder,
        &paths,
        &computer_of(&id, true),
        true,
        idle(),
    )
    .unwrap();
    assert_eq!(status["state"], "ready");
    assert!(recorder.scripts()[0].contains("silo-computer-use apply --approval auto --force"));
    assert_eq!(settings(&paths, &id).applied, Some(Approval::Auto));
    let broken = Recorder::new("not json");
    assert!(setup_with(
        test_gate(),
        &broken,
        &paths,
        &computer_of(&id, true),
        false,
        idle()
    )
    .is_err());
    assert_eq!(
        settings(&paths, &id).last.unwrap().reason.as_deref(),
        Some("invalid-report")
    );
}

// ------------------------------------------------------- boot reconcile

#[test]
fn app_start_applies_where_the_last_attempt_is_missing_failed_or_for_another_mode() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    let guest = Guest::new(&id);
    let runner: SharedRunner = guest.clone();
    let names = ["dev".to_owned()];
    let gate = test_gate();
    let reconcile = |gate| -> usize {
        let handles = reconcile_in(gate, &runner, &paths, &names);
        let count = handles.len();
        for handle in handles {
            handle.join().unwrap();
        }
        count
    };
    // Nothing is known (an import, a fork, a lost record): applied.
    assert_eq!(reconcile(gate), 1);
    assert_eq!(guest.modes(), ["ask"]);
    // Everything is current: nothing runs, and the guest is not even asked.
    let inspects = guest.inspects.lock().unwrap().0;
    assert_eq!(reconcile(gate), 0);
    assert_eq!(guest.inspects.lock().unwrap().0, inspects);
    // The user chose auto and the app quit before the apply ran.
    set_approval(&paths, &id, Approval::Auto).unwrap();
    assert_eq!(reconcile(gate), 1);
    assert_eq!(guest.modes(), ["ask", "auto"]);
    // A failed or partial attempt is retried, and so is one the quit cut short.
    for reply in [
        Reply::Report("failed", Some("setup-failed")),
        Reply::Report("partial", Some("setup-partial")),
        Reply::TimedOut,
    ] {
        set_approval(&paths, &id, Approval::Ask).unwrap();
        guest.plan([reply]);
        assert_eq!(reconcile(gate), 1);
        assert!(read_policy(&paths, &id).needs_apply());
        assert_eq!(reconcile(gate), 1, "retried while it keeps failing");
        assert_eq!(settings(&paths, &id).applied, Some(Approval::Ask));
        set_approval(&paths, &id, Approval::Auto).unwrap();
        assert_eq!(reconcile(gate), 1);
    }
    // A computer that is not built in is left alone.
    write_computers(&paths, false);
    assert_eq!(reconcile(gate), 0);
}

#[test]
fn the_initial_mode_comes_from_the_app_setting_and_only_true_means_auto() {
    let setting = |value: serde_json::Value| {
        let mut map = serde_json::Map::new();
        map.insert("computerUseAutoApproval".into(), value);
        initial_approval_from(&map)
    };
    assert_eq!(
        initial_approval_from(&serde_json::Map::new()),
        Approval::Ask
    );
    assert_eq!(setting(json!(false)), Approval::Ask);
    assert_eq!(setting(json!("true")), Approval::Ask);
    assert_eq!(setting(json!(true)), Approval::Auto);
}

#[test]
fn a_new_computer_starts_with_the_initial_mode_and_no_attempt() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    start_with(&paths, &id, Approval::Auto);
    let fresh = settings(&paths, &id);
    assert_eq!(fresh.approval, Approval::Auto);
    assert_eq!((fresh.applied, fresh.last), (None, None));
    let guest = Guest::new(&id);
    write_computers_of(&paths, &id);
    boot_of(test_gate(), &guest, &paths)
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(guest.modes(), ["auto"]);
}

#[test]
fn an_import_takes_the_local_initial_mode_whatever_the_archive_or_an_earlier_computer_had() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    let import = |mode| {
        with_initial_approval(mode, || {
            runtime::checkpoints::import_pending_restore(
                &paths,
                &id,
                "silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9",
                "silo-backup-0-330418-1790360984903",
            )
            .unwrap()
        })
    };
    import(Approval::Auto);
    assert_eq!(settings(&paths, &id).approval, Approval::Auto);
    // An earlier computer of this id that had auto does not carry it over when the setting is off.
    set_approval(&paths, &id, Approval::Auto).unwrap();
    import(Approval::Ask);
    let imported = settings(&paths, &id);
    assert_eq!(imported.approval, Approval::Ask);
    assert_eq!((imported.applied, imported.last), (None, None));
}

#[test]
fn the_boot_applies_what_an_imported_or_forked_disk_does_not_have() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    // A computer of this id had auto applied here; importing it again starts from ask.
    set_approval(&paths, &id, Approval::Auto).unwrap();
    record_attempt(&paths, &id, attempt(Approval::Auto, Outcome::Applied, None));
    runtime::checkpoints::import_pending_restore(
        &paths,
        &id,
        "silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9",
        "silo-backup-0-330418-1790360984903",
    )
    .unwrap();
    let imported = settings(&paths, &id);
    assert_eq!(imported.approval, Approval::Ask);
    assert_eq!((imported.applied, imported.last), (None, None));
    let guest = Guest::new(&id);
    boot_of(test_gate(), &guest, &paths)
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(
        guest.modes(),
        ["ask"],
        "the destination's default, not the imported disk's"
    );
    assert_eq!(settings(&paths, &id).applied, Some(Approval::Ask));
    // A fork inherits its source's choice and applies it at its own first boot.
    let child = computer();
    set_approval(&paths, &id, Approval::Auto).unwrap();
    inherit_settings(&paths, &id, &child).unwrap();
    write_computers_of(&paths, &child);
    let guest = Guest::new(&child);
    boot_of(test_gate(), &guest, &paths)
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(guest.modes(), ["auto"]);
    assert_eq!(settings(&paths, &child).applied, Some(Approval::Auto));
}

// --------------------------------------------------------- state mapping

fn state(inputs: Inputs) -> Value {
    computer_use_state(&inputs).0
}

fn defaults() -> Settings {
    Settings::default()
}

fn ready() -> Status {
    Status::Ready {
        path: PathBuf::from("/x"),
        version: "26.928.31416".into(),
    }
}

fn guest_status(state: &str, reason: Option<&str>) -> Value {
    json!({"state":state,"reason":reason,"compatibility":"tested","warning":null,
        "appVersion":"26.928.31416","runtimeVersion":"0.0.27/20260927214556-b77d38801cca",
        "lcuVersion":"0.8.0","agents":["claude-code"],"mount":"ok"})
}

#[test]
fn app_status_maps_to_the_computer_use_state() {
    let settings = defaults();
    let map = |app: Option<&Status>| {
        state(Inputs {
            app,
            computer_running: true,
            guest: None,
            settings: &settings,
            pending: false,
            retrying: false,
        })
    };
    assert_eq!(map(None)["state"], "preparing");
    assert_eq!(map(Some(&Status::Idle))["state"], "preparing");
    for status in [
        Status::Downloading {
            received_bytes: 1,
            total_bytes: 2,
        },
        Status::Verifying,
        Status::Extracting,
    ] {
        assert_eq!(map(Some(&status))["state"], "preparing");
    }
    let failed = map(Some(&Status::Failed {
        reason: "No space left.".into(),
        retryable: true,
    }));
    // Silo retries by itself, so a retryable failure is still "preparing".
    assert_eq!(failed["state"], "preparing");
    assert!(failed["reason"]
        .as_str()
        .unwrap()
        .starts_with("No space left."));
    let final_failure = map(Some(&Status::Failed {
        reason: "The checksum did not match.".into(),
        retryable: false,
    }));
    assert_eq!(
        (
            final_failure["state"].as_str(),
            final_failure["reason"].as_str()
        ),
        (Some("failed"), Some("The checksum did not match."))
    );
    assert_eq!(final_failure["cause"], "app-download");
    assert!(failed.get("cause").is_none());
    // Ready app, running computer, no helper yet.
    let waiting = map(Some(&ready()));
    assert_eq!(waiting["state"], "unavailable");
    assert!(waiting["reason"]
        .as_str()
        .unwrap()
        .contains("not set up yet"));
}

#[test]
fn the_confirmed_approval_is_reported_whatever_the_download_state() {
    // The last mode that was applied completely is the host's own record: it never
    // disappears because the ChatGPT app is downloading, failed to download or is unknown.
    let settings = Settings {
        approval: Approval::Ask,
        applied: Some(Approval::Auto),
        last: Some(attempt(Approval::Auto, Outcome::Applied, None)),
        ..Settings::default()
    };
    let statuses = [
        None,
        Some(Status::Idle),
        Some(Status::Downloading {
            received_bytes: 1,
            total_bytes: 2,
        }),
        Some(Status::Verifying),
        Some(Status::Extracting),
        Some(Status::Failed {
            reason: "No space left.".into(),
            retryable: true,
        }),
        Some(Status::Failed {
            reason: "The checksum did not match.".into(),
            retryable: false,
        }),
        Some(ready()),
    ];
    for app in statuses {
        for running in [false, true] {
            let value = state(Inputs {
                app: app.as_ref(),
                computer_running: running,
                guest: None,
                settings: &settings,
                pending: false,
                retrying: false,
            });
            assert_eq!(value["approval"], "ask", "{app:?} {running}");
            assert_eq!(value["appliedApproval"], "auto", "{app:?} {running}");
            assert_eq!(value["approvalApply"], "pending", "{app:?} {running}");
        }
    }
}

#[test]
fn guest_status_maps_to_the_contract_fields() {
    let settings = Settings {
        approval: Approval::Auto,
        applied: Some(Approval::Auto),
        last: Some(attempt(Approval::Auto, Outcome::Applied, None)),
        ..Settings::default()
    };
    let app = ready();
    let map = |guest: Value| {
        computer_use_state(&Inputs {
            app: Some(&app),
            computer_running: true,
            guest: Some(&guest),
            settings: &settings,
            pending: false,
            retrying: false,
        })
    };
    let (value, remembered) = map(guest_status("ready", None));
    assert_eq!(
        value,
        json!({
            "state": "ready", "reason": null, "compatibility": "tested", "warning": null,
            "approval": "auto", "appliedApproval": "auto", "approvalApply": "applied",
            "approvalApplyReason": null, "appVersion": "26.928.31416",
            "runtimeVersion": "0.0.27/20260927214556-b77d38801cca",
            "lcuVersion": "0.8.0", "agents": ["claude-code"],
        })
    );
    assert_eq!(remembered.unwrap().state, "ready");
    let (installing, remembered) = map(guest_status("installing", None));
    assert_eq!(installing["state"], "installing");
    assert!(remembered.is_none(), "transient states are not remembered");
    let (needs_app, _) = map(guest_status("needs-app", Some("app-missing")));
    assert_eq!(needs_app["state"], "preparing");
    let (failed, remembered) = map(guest_status("failed", Some("doctor-failed")));
    assert_eq!(failed["state"], "failed");
    assert!(failed["reason"]
        .as_str()
        .unwrap()
        .contains("readiness check"));
    assert_eq!(remembered.unwrap().state, "failed");
    let (not_set_up, _) = map(guest_status("not-set-up", Some("not-configured")));
    assert_eq!(not_set_up["state"], "unavailable");
    // An untested pair is shown with its warning, not blocked.
    let mut untested = guest_status("ready", None);
    untested["compatibility"] = json!("untested");
    untested["warning"] = json!("Not tested with this app.");
    let (untested, _) = map(untested);
    assert_eq!(untested["compatibility"], "untested");
    assert_eq!(untested["warning"], "Not tested with this app.");
    // An unknown compatibility value never reaches the UI.
    let mut odd = guest_status("ready", None);
    odd["compatibility"] = json!("surprise");
    assert_eq!(map(odd).0["compatibility"], "unknown");
}

#[test]
fn what_the_guest_reports_about_approval_never_changes_what_the_host_reports() {
    // Status reads never change policy and never decide it: a guest status that claims
    // another mode (a stale or forged record) is ignored.
    let settings = Settings {
        approval: Approval::Ask,
        applied: Some(Approval::Ask),
        last: Some(attempt(Approval::Ask, Outcome::Applied, None)),
        ..Settings::default()
    };
    let app = ready();
    let mut guest = guest_status("ready", None);
    guest["approval"] = json!("auto");
    guest["approvalRevision"] = json!(u64::MAX);
    guest["approvalGeneration"] = json!("someone-else");
    guest["approvalConfirmed"] = json!(true);
    let value = state(Inputs {
        app: Some(&app),
        computer_running: true,
        guest: Some(&guest),
        settings: &settings,
        pending: false,
        retrying: false,
    });
    assert_eq!(value["state"], "ready");
    assert_eq!(
        (
            value["approval"].as_str(),
            value["appliedApproval"].as_str(),
            value["approvalApply"].as_str()
        ),
        (Some("ask"), Some("ask"), Some("applied"))
    );
}

#[test]
fn approval_apply_follows_the_last_attempt_for_the_chosen_mode() {
    let with = |approval, applied, last: Option<Attempt>| Settings {
        approval,
        applied,
        last,
        ..Settings::default()
    };
    let apply = |settings: &Settings, pending| approval_apply(settings, pending);
    assert_eq!(apply(&defaults(), false), "pending", "nothing known yet");
    let applied = with(
        Approval::Ask,
        Some(Approval::Ask),
        Some(attempt(Approval::Ask, Outcome::Applied, None)),
    );
    assert_eq!(apply(&applied, false), "applied");
    assert_eq!(apply(&applied, true), "pending", "a scheduled apply wins");
    let failed = with(
        Approval::Auto,
        Some(Approval::Ask),
        Some(attempt(Approval::Auto, Outcome::Failed, Some("x"))),
    );
    assert_eq!(apply(&failed, false), "failed");
    let partial = with(
        Approval::Auto,
        Some(Approval::Ask),
        Some(attempt(Approval::Auto, Outcome::Partial, Some("x"))),
    );
    assert_eq!(apply(&partial, false), "partial");
    // An attempt for another mode says nothing about this one.
    let other = with(
        Approval::Ask,
        Some(Approval::Ask),
        Some(attempt(Approval::Auto, Outcome::Failed, Some("x"))),
    );
    assert_eq!(apply(&other, false), "pending");
    assert!(other.last.is_some());
}

#[test]
fn every_failure_code_has_a_message_and_mount_problems_are_explained() {
    for code in [
        "interrupted",
        "doctor-failed",
        "desktop-session-not-running",
        "timed-out",
        "lcu-archive-unavailable",
        "lcu-archive-mismatch",
        "mount-missing",
        "mount-writable",
        "command-failed",
        "anything-else",
    ] {
        assert!(!reason_text(code).is_empty(), "{code}");
    }
    for code in [
        "cancelled",
        "timed-out",
        "unreachable",
        "invalid-report",
        "setup-partial",
        "setup-failed",
        "mount-missing",
        "anything-else",
    ] {
        assert!(!approval_reason_text(code).is_empty(), "{code}");
        assert!(!approval_reason_text(code).contains(code), "{code}");
    }
    assert!(reason_text("mount-missing").contains("new computer"));
    assert!(!reason_text("anything-else").contains("anything-else"));
}

#[test]
fn a_stopped_computer_keeps_its_approval_and_last_known_versions() {
    let app = ready();
    let settings = Settings {
        approval: Approval::Auto,
        known: Some(Known {
            state: "ready".into(),
            compatibility: Some("untested".into()),
            warning: Some("Not tested.".into()),
            app_version: Some("26.928.31416".into()),
            runtime_version: Some("0.0.27/20260927214556-b77d38801cca".into()),
            lcu_version: Some("0.8.0".into()),
            agents: Some(vec!["codex".into()]),
        }),
        ..Settings::default()
    };
    let (value, remembered) = computer_use_state(&Inputs {
        app: Some(&app),
        computer_running: false,
        guest: None,
        settings: &settings,
        pending: false,
        retrying: false,
    });
    assert_eq!(value["state"], "ready");
    assert_eq!(value["approval"], "auto");
    assert_eq!(value["appVersion"], "26.928.31416");
    assert_eq!(value["lcuVersion"], "0.8.0");
    assert_eq!(value["compatibility"], "untested");
    assert_eq!(value["agents"], json!(["codex"]));
    assert!(remembered.is_none());
    // Nothing known yet: unavailable, with the approval still shown.
    let (fresh, _) = computer_use_state(&Inputs {
        app: Some(&app),
        computer_running: false,
        guest: None,
        settings: &Settings {
            approval: Approval::Auto,
            ..Settings::default()
        },
        pending: false,
        retrying: false,
    });
    assert_eq!(fresh["state"], "unavailable");
    assert_eq!(fresh["approval"], "auto");
    // App problems still win for a stopped computer.
    let (download, _) = computer_use_state(&Inputs {
        app: Some(&Status::Failed {
            reason: "The checksum did not match.".into(),
            retryable: false,
        }),
        computer_running: false,
        guest: None,
        settings: &settings,
        pending: false,
        retrying: false,
    });
    assert_eq!(download["state"], "failed");
    assert_eq!(download["approval"], "auto");
}

#[test]
fn desktop_state_is_reported_only_for_built_in_computers() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    assert!(desktop_state(&paths, &computer_configuration(false), true, None).is_none());
    let value = desktop_state(&paths, &computer_configuration(true), false, None).unwrap();
    assert_eq!(value["approval"], "ask");
    assert!(value["state"].is_string());
}

#[test]
fn a_ready_report_is_remembered_for_the_stopped_computer() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let app = ready();
    let report = guest_status("ready", None);
    let current = settings(&paths, COMPUTER_ID);
    let (_, remembered) = computer_use_state(&Inputs {
        app: Some(&app),
        computer_running: true,
        guest: Some(&report),
        settings: &current,
        pending: false,
        retrying: false,
    });
    remember(&paths, COMPUTER_ID, remembered.unwrap());
    let known = settings(&paths, COMPUTER_ID).known.unwrap();
    assert_eq!(known.lcu_version.as_deref(), Some("0.8.0"));
    assert_eq!(known.agents, Some(vec!["claude-code".to_owned()]));
}

// ------------------------------------------- policy vs observation (races)

#[test]
fn status_reads_never_rewrite_the_policy() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    set_approval(&paths, &id, Approval::Auto).unwrap();
    record_attempt(
        &paths,
        &id,
        attempt(Approval::Auto, Outcome::Failed, Some("setup-failed")),
    );
    let before = fs::read(policy_path(&paths, &id).unwrap()).unwrap();
    let report = guest_status("ready", None);
    for running in [false, true] {
        for _ in 0..3 {
            let value =
                desktop_state(&paths, &computer_of(&id, true), running, Some(&report)).unwrap();
            assert_eq!(value["approval"], "auto");
        }
    }
    assert_eq!(fs::read(policy_path(&paths, &id).unwrap()).unwrap(), before);
    // Saving what the guest reported touches only the observation.
    remember(
        &paths,
        &id,
        Known {
            state: "ready".into(),
            lcu_version: Some("0.8.3".into()),
            ..Known::default()
        },
    );
    assert_eq!(fs::read(policy_path(&paths, &id).unwrap()).unwrap(), before);
}

#[test]
fn recording_an_attempt_never_loses_the_users_choice() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let rounds = 150;
    std::thread::scope(|scope| {
        for reader in 0..3 {
            let paths = &paths;
            scope.spawn(move || {
                for round in 0..rounds {
                    // What an apply does when it ends, for whatever mode it ran.
                    let mode = if (reader + round) % 2 == 0 {
                        Approval::Auto
                    } else {
                        Approval::Ask
                    };
                    record_attempt(paths, COMPUTER_ID, attempt(mode, Outcome::Applied, None));
                    remember(
                        paths,
                        COMPUTER_ID,
                        Known {
                            state: "ready".into(),
                            app_version: Some(format!("{reader}-{round}")),
                            ..Known::default()
                        },
                    );
                }
            });
        }
        let paths = &paths;
        scope.spawn(move || {
            for round in 0..rounds {
                let approval = if round % 2 == 0 {
                    Approval::Auto
                } else {
                    Approval::Ask
                };
                set_approval(paths, COMPUTER_ID, approval).unwrap();
            }
        });
    });
    // Round 149 was the last change: `ask`, whatever the attempts recorded meanwhile.
    let policy = read_policy(&paths, COMPUTER_ID);
    assert_eq!(policy.approval, Approval::Ask);
    assert!(policy.last.is_some() && policy.applied.is_some());
    assert!(settings(&paths, COMPUTER_ID).known.is_some());
}

#[test]
fn an_unreadable_policy_is_unknown_not_ask_until_an_apply_replaces_it_with_the_default() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    // No policy yet: the default ask is the real choice.
    assert!(!settings(&paths, &id).unreadable);
    let value = answer_of(&paths, &id, false);
    assert_eq!(value["approval"], "ask");
    // A policy file that exists but cannot be parsed is not a choice of ask.
    set_approval(&paths, &id, Approval::Auto).unwrap();
    fs::write(policy_path(&paths, &id).unwrap(), b"{not json").unwrap();
    assert!(settings(&paths, &id).unreadable);
    let value = answer_of(&paths, &id, false);
    assert_eq!(value["approval"], "unknown");
    // A recorded attempt never overwrites a file it cannot read.
    record_attempt(&paths, &id, attempt(Approval::Ask, Outcome::Applied, None));
    assert!(settings(&paths, &id).unreadable);
    // Fail closed: the next apply drives the guest to ask and restores a readable policy.
    let guest = Guest::new(&id);
    boot_of(test_gate(), &guest, &paths)
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(guest.modes(), ["ask"]);
    let repaired = settings(&paths, &id);
    assert!(!repaired.unreadable);
    assert_eq!(
        (repaired.approval, repaired.applied),
        (Approval::Ask, Some(Approval::Ask))
    );
}

// ------------------------------------------------------------- identity

/// A computer that Silo replaced (deleted and created again under the same name) while the
/// apply waited for its turn is never touched; the replacement is recognised by its
/// runtime instance, not by its name.
#[test]
fn a_computer_replaced_after_inspection_is_not_applied_once_the_apply_gets_its_turn() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    let guest = Guest::new(&id);
    let gate = test_gate();
    // A delete-and-create holds the computer's turn when the apply is scheduled.
    let replacing = gate.computer(&id, "dev", "Recreating dev").unwrap();
    let handle = boot_of(gate, &guest, &paths).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        !handle.is_finished(),
        "the apply waits for the computer's turn"
    );
    assert!(guest.runs.lock().unwrap().is_empty());
    // The replacement is another runtime instance by the time the turn ends.
    guest.inspects.lock().unwrap().1 = Some(1);
    drop(replacing);
    handle.join().unwrap();
    assert!(guest.runs.lock().unwrap().is_empty());
    assert_eq!(
        settings(&paths, &id).last,
        None,
        "nothing was recorded either"
    );
}

#[test]
fn the_helper_runs_inside_the_computers_turn() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    let guest = Guest::new(&id);
    let (entered, entered_receiver) = std::sync::mpsc::channel();
    let release = Arc::new(std::sync::Barrier::new(2));
    *guest.stall.lock().unwrap() = Some((entered, release.clone()));
    let gate = test_gate();
    let handle = boot_of(gate, &guest, &paths).unwrap();
    entered_receiver
        .recv_timeout(Duration::from_secs(10))
        .expect("the helper reached the guest");
    // Other work on the computer cannot start while the helper is in the guest.
    assert_eq!(
        gate.try_computer(&id, "dev", "Checkpointing dev").err(),
        Some(runtime::operation_gate::GateError::Busy)
    );
    release.wait();
    handle.join().unwrap();
    assert!(gate.try_computer(&id, "dev", "Checkpointing dev").is_ok());
}

#[test]
fn an_identity_that_cannot_be_established_at_boot_is_never_applied_later() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    write_computers(&paths, true);
    let running = |config: Value, instance: Option<&str>| {
        let mut value = json!({"name":"dev","status":"Running","config":config});
        if let Some(instance) = instance {
            value["runtime_instance_id"] = json!(instance);
        }
        value
    };
    /// Answers `inspect` with the next prepared reply (the last one repeats).
    struct Sequence(StdMutex<Vec<Option<String>>>, Recorder);
    impl RuntimeRunner for Sequence {
        fn run(
            &self,
            paths: &RuntimePaths,
            args: &[String],
            timeout: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            if args.first().is_some_and(|a| a == "inspect") {
                let mut replies = self.0.lock().unwrap();
                let reply = if replies.len() > 1 {
                    replies.remove(0)
                } else {
                    replies[0].clone()
                };
                return reply
                    .map(|stdout| CommandOutput {
                        stdout,
                        stderr: String::new(),
                    })
                    .ok_or_else(|| RuntimeError::Unavailable("runtime busy".into()));
            }
            self.1.run(paths, args, timeout)
        }
    }
    let labelled = json!({"labels":{"silo.machine-id":COMPUTER_ID}});
    let good = running(labelled.clone(), Some("one")).to_string();
    let failing = || None;
    let cases = [
        // The first inspection failed; a later one would show a running instance.
        vec![failing(), Some(good.clone())],
        // No instance id, an unlabelled computer, and another computer's label.
        vec![
            Some(running(labelled.clone(), None).to_string()),
            Some(good.clone()),
        ],
        vec![
            Some(running(json!({}), Some("one")).to_string()),
            Some(good.clone()),
        ],
        vec![
            Some(
                running(
                    json!({"labels":{"silo.machine-id":"someone-else"}}),
                    Some("one"),
                )
                .to_string(),
            ),
            Some(good.clone()),
        ],
    ];
    for replies in cases {
        let runner = Arc::new(Sequence(StdMutex::new(replies), Recorder::new("")));
        assert!(apply_with(test_gate(), runner.clone(), &paths, "dev", Trigger::Boot).is_none());
        assert!(runner.1.calls.lock().unwrap().is_empty());
    }
    // Another computer with the same name inside the turn: the label differs, nothing runs.
    let runner = Arc::new(Sequence(
        StdMutex::new(vec![
            Some(good.clone()),
            Some(
                running(
                    json!({"labels":{"silo.machine-id":"someone-else"}}),
                    Some("one"),
                )
                .to_string(),
            ),
        ]),
        Recorder::new(""),
    ));
    apply_with(test_gate(), runner.clone(), &paths, "dev", Trigger::Boot)
        .unwrap()
        .join()
        .unwrap();
    assert!(runner.1.calls.lock().unwrap().is_empty());
}

/// Inspect output of a running built-in computer. `instance` is the reported `runtime_instance_id`;
/// `None` leaves the entry out, as a runtime without Silo's patch does.
fn inspected_instance(instance: Option<&str>) -> String {
    let mut value = json!({"name":"dev","status":"Running",
        "config":{"labels":{"silo.machine-id":COMPUTER_ID}}});
    if let Some(instance) = instance {
        value["runtime_instance_id"] = json!(instance);
    }
    value.to_string()
}

/// A restart between the boot and the launch (a new runtime instance) is refused; the same
/// instance still sets computer use up.
#[test]
fn a_restart_between_the_boot_and_the_launch_is_refused() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    write_computers(&paths, true);
    struct Sequence(StdMutex<Vec<&'static str>>, Recorder);
    impl RuntimeRunner for Sequence {
        fn run(
            &self,
            paths: &RuntimePaths,
            args: &[String],
            timeout: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            if args.first().is_some_and(|a| a == "inspect") {
                let mut instances = self.0.lock().unwrap();
                let instance = if instances.len() > 1 {
                    instances.remove(0)
                } else {
                    instances[0]
                };
                return Ok(CommandOutput {
                    stdout: inspected_instance(Some(instance)),
                    stderr: String::new(),
                });
            }
            self.1.run(paths, args, timeout)
        }
    }
    let same = Arc::new(Sequence(StdMutex::new(vec!["1:a"]), Recorder::new("")));
    apply_with(test_gate(), same.clone(), &paths, "dev", Trigger::Boot)
        .unwrap()
        .join()
        .unwrap();
    assert!(!same.1.calls.lock().unwrap().is_empty());
    let restarted = Arc::new(Sequence(
        StdMutex::new(vec!["1:a", "2:b"]),
        Recorder::new(""),
    ));
    apply_with(test_gate(), restarted.clone(), &paths, "dev", Trigger::Boot)
        .unwrap()
        .join()
        .unwrap();
    assert!(restarted.1.calls.lock().unwrap().is_empty());
}

/// A runtime whose inspect output has no `runtime_instance_id` (Silo's patch missing, as on
/// the 0.7.6 build that shipped without it) cannot establish an identity: computer use
/// never runs against it and the cause is reported, not skipped silently.
#[test]
fn a_runtime_that_reports_no_instance_id_never_runs_the_helper() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    write_computers(&paths, true);
    struct Unpatched(Recorder);
    impl RuntimeRunner for Unpatched {
        fn run(
            &self,
            paths: &RuntimePaths,
            args: &[String],
            timeout: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            if args.first().is_some_and(|a| a == "inspect") {
                return Ok(CommandOutput {
                    stdout: inspected_instance(None),
                    stderr: String::new(),
                });
            }
            self.0.run(paths, args, timeout)
        }
    }
    let runner = Arc::new(Unpatched(Recorder::new("")));
    assert!(apply_with(test_gate(), runner.clone(), &paths, "dev", Trigger::Boot).is_none());
    assert!(runner.0.calls.lock().unwrap().is_empty());
}

#[test]
fn a_stopped_computer_is_left_alone() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    write_computers(&paths, true);
    struct Stopped;
    impl RuntimeRunner for Stopped {
        fn run(
            &self,
            _: &RuntimePaths,
            args: &[String],
            _: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            assert_eq!(args[0], "inspect", "{args:?}");
            Ok(CommandOutput {
                stdout: json!({"name":"dev","status":"Stopped","config":{}}).to_string(),
                stderr: String::new(),
            })
        }
    }
    assert!(apply_with(test_gate(), Arc::new(Stopped), &paths, "dev", Trigger::Boot).is_none());
}

#[test]
fn the_shared_folder_is_prepared_again_when_the_start_up_attempt_failed() {
    let _state = crate::test_support::global_state();
    reset_published_for_test();
    set_test_published_dir(None);
    let directory = tempfile::tempdir().unwrap();
    // Start-up could not create the folders (a plain file is in the way).
    let blocker = directory.path().join("blocked");
    fs::write(&blocker, b"x").unwrap();
    let root = blocker.join("chatgpt");
    assert!(register_published(&root).is_err());
    assert!(mount_args(&computer_configuration(true)).is_err());
    // The cause goes away; neither a restart nor a preparation attempt has run yet:
    // the next computer that needs the folder prepares it itself.
    fs::remove_file(&blocker).unwrap();
    let args = mount_args(&computer_configuration(true)).unwrap();
    let dir = published_dir().expect("the folder is registered");
    assert!(
        dir.ends_with("chatgpt/published") && dir.is_dir(),
        "{dir:?}"
    );
    assert_eq!(
        args,
        [
            "-v",
            &format!("{}:{GUEST_MOUNT}:ro,uid=0,gid=0", dir.display())
        ]
    );
    // A preparation attempt registers it as well (the worker calls this every time).
    reset_published_for_test();
    let dir = register_published(&directory.path().join("chatgpt")).unwrap();
    assert_eq!(published_dir(), Some(dir));
    reset_published_for_test();
}

// ------------------------------------------------- review follow-ups

#[test]
fn choosing_ask_while_auto_applies_converges_even_after_a_successful_ask() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    let guest = Guest::new(&id);
    let gate = test_gate();
    // Ask is applied successfully first.
    boot_of(gate, &guest, &paths).unwrap().join().unwrap();
    assert_eq!(settings(&paths, &id).applied, Some(Approval::Ask));
    let configuration = computer_of(&id, true);
    let (entered, entered_receiver) = std::sync::mpsc::channel();
    let release = Arc::new(std::sync::Barrier::new(2));
    *guest.stall.lock().unwrap() = Some((entered, release.clone()));
    let auto = apply_approval_in(
        gate,
        guest.clone(),
        &paths,
        &configuration,
        Approval::Auto,
        true,
    )
    .unwrap()
    .unwrap();
    entered_receiver
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    // Back to ask while auto is still running. The ask result from before must not leave
    // the choice pending: whether a follow-up is scheduled or not, the running turn
    // converges on the current choice.
    let back = apply_approval_in(
        gate,
        guest.clone(),
        &paths,
        &configuration,
        Approval::Ask,
        true,
    )
    .unwrap();
    release.wait();
    auto.join().unwrap();
    if let Some(back) = back {
        back.join().unwrap();
    }
    assert_eq!(guest.modes(), ["ask", "auto", "ask"]);
    assert_eq!(guest.configured().as_deref(), Some("ask"));
    let state = answer_of(&paths, &id, true);
    assert_eq!(
        (
            state["approval"].as_str(),
            state["appliedApproval"].as_str(),
            state["approvalApply"].as_str()
        ),
        (Some("ask"), Some("ask"), Some("applied"))
    );
    assert!(!is_pending(&id));
}

#[test]
fn a_delete_of_the_computer_cancels_a_running_apply_and_another_computers_delete_does_not() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    let (guest, entered) = hanging(&id);
    let gate = test_gate();
    let handle = apply_with(gate, guest, &paths, "dev", Trigger::Boot).unwrap();
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    let other = computer();
    let queued = std::thread::spawn(move || {
        drop(gate.removing(&[other], "Deleting other").unwrap());
    });
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        !handle.is_finished(),
        "another computer's delete leaves it alone"
    );
    let started = std::time::Instant::now();
    drop(gate.removing(&[id.clone()], "Deleting dev").unwrap());
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "delete waited {:?}",
        started.elapsed()
    );
    handle.join().unwrap();
    queued.join().unwrap();
    assert_eq!(
        settings(&paths, &id).last.unwrap().reason.as_deref(),
        Some("cancelled")
    );
}

/// Runs the manual setup on its own thread inside the computer's cancellable turn, like the
/// desktop action does.
fn manual_setup(
    gate: &'static runtime::operation_gate::OperationGate,
    guest: Arc<Hanging>,
    paths: RuntimePaths,
    id: String,
) -> std::thread::JoinHandle<Result<Value, RuntimeError>> {
    std::thread::spawn(move || {
        let turn = gate.computer(&id, "dev", "Updating dev desktop").unwrap();
        turn.allow_cancel();
        setup_with(
            gate,
            guest.as_ref(),
            &paths,
            &computer_of(&id, true),
            true,
            turn.cancel_token(),
        )
    })
}

#[test]
fn manual_setup_is_cancelled_by_a_stop_of_the_computer_and_by_quit() {
    for shutdown in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let id = computer();
        write_computers_of(&paths, &id);
        let (guest, entered) = hanging(&id);
        let gate = test_gate();
        let handle = manual_setup(gate, guest, paths.clone(), id.clone());
        entered.recv_timeout(Duration::from_secs(10)).unwrap();
        let started = std::time::Instant::now();
        let waiting = if shutdown {
            gate.kind(runtime::operation_gate::OperationKind::Shutdown)
                .device("Quitting")
                .unwrap()
        } else {
            gate.kind(runtime::operation_gate::OperationKind::Lifecycle)
                .computer(&id, "dev", "Stopping dev")
                .unwrap()
        };
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "waited {:?}",
            started.elapsed()
        );
        drop(waiting);
        assert!(matches!(
            handle.join().unwrap(),
            Err(RuntimeError::Cancelled { .. })
        ));
        assert_eq!(
            settings(&paths, &id).last.unwrap().reason.as_deref(),
            Some("cancelled")
        );
    }
}

#[test]
fn an_attempt_that_never_ended_is_applied_again_even_when_the_last_result_matches() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    set_approval(&paths, &id, Approval::Auto).unwrap();
    let guest = Guest::new(&id);
    let gate = test_gate();
    boot_of(gate, &guest, &paths).unwrap().join().unwrap();
    assert!(!read_policy(&paths, &id).needs_apply());
    // A forced setup (a new harness was installed) dies before it records a result.
    struct Crashing;
    impl RuntimeRunner for Crashing {
        fn run(
            &self,
            _: &RuntimePaths,
            _: &[String],
            _: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            panic!("the app was killed");
        }
    }
    let configuration = computer_of(&id, true);
    let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = setup_with(
            gate,
            &Crashing,
            &paths,
            &configuration,
            true,
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
    }));
    assert!(crashed.is_err());
    let stored = read_policy(&paths, &id);
    assert_eq!(stored.last.as_ref().unwrap().outcome, Outcome::Applied);
    assert_eq!(stored.unfinished, Some(Approval::Auto));
    assert!(stored.needs_apply());
    assert_eq!(answer_of(&paths, &id, false)["approvalApply"], "pending");
    // The next app start applies it again, and the result replaces the marker.
    let runner: SharedRunner = guest.clone();
    let handles = reconcile_in(gate, &runner, &paths, &["dev".to_owned()]);
    assert_eq!(handles.len(), 1);
    for handle in handles {
        handle.join().unwrap();
    }
    assert_eq!(guest.modes(), ["auto", "auto"]);
    let stored = read_policy(&paths, &id);
    assert_eq!(stored.unfinished, None);
    assert!(!stored.needs_apply());
    assert_eq!(answer_of(&paths, &id, true)["approvalApply"], "applied");
}

#[test]
fn a_run_that_was_not_an_attempt_leaves_the_unfinished_marker_as_it_was() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    let guest = Guest::new(&id);
    guest.plan([Reply::Report("failed", Some("app-missing"))]);
    boot_of(test_gate(), &guest, &paths)
        .unwrap()
        .join()
        .unwrap();
    let stored = read_policy(&paths, &id);
    assert_eq!((stored.unfinished, stored.last), (None, None));
}

#[test]
fn the_runtimes_exec_timeout_is_a_timed_out_attempt_not_an_unreachable_computer() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    set_approval(&paths, &id, Approval::Auto).unwrap();
    let guest = Guest::new(&id);
    // MicroSandbox's `msb exec --timeout 900s` ends this way (drive_stream in exec.rs).
    guest.plan([Reply::Failed("Error: exec timed out after 900s")]);
    boot_of(test_gate(), &guest, &paths)
        .unwrap()
        .join()
        .unwrap();
    let last = settings(&paths, &id).last.unwrap();
    assert_eq!(
        (last.outcome, last.reason.as_deref()),
        (Outcome::Failed, Some("timed-out"))
    );
    // Other failures of the command stay unreachable.
    for detail in [
        "exec session ended without exit event",
        "the computer timed out after 5s",
    ] {
        guest.plan([Reply::Failed(detail)]);
        boot_of(test_gate(), &guest, &paths)
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(
            settings(&paths, &id).last.unwrap().reason.as_deref(),
            Some("unreachable"),
            "{detail}"
        );
    }
}

#[test]
fn only_a_stop_or_restart_key_preempts_the_helper() {
    let id = computer();
    for (key, preempts) in [
        (format!("computer:{id}:stop"), true),
        (format!("computer:{id}:restart"), true),
        (format!("computer:{id}:start"), false),
        (format!("computer:{id}:dismiss-error"), false),
        (format!("computer:{id}:modify"), false),
        // Another computer's key and a malformed one never name this computer's stop.
        (format!("computer:{}:stop", computer()), false),
        (format!("computer:{id}stop"), false),
    ] {
        assert_eq!(lifecycle_key_preempts(Some(&key), &id), preempts, "{key}");
    }
    assert!(
        lifecycle_key_preempts(None, &id),
        "an unnamed action is safe-sided"
    );
}

fn queue_keyed(
    gate: &'static runtime::operation_gate::OperationGate,
    id: &str,
    action: &str,
) -> std::thread::JoinHandle<()> {
    let (id, action) = (id.to_owned(), action.to_owned());
    std::thread::spawn(move || {
        drop(
            gate.kind(runtime::operation_gate::OperationKind::Lifecycle)
                .acquire(
                    runtime::operation_gate::Scope::Computer { id: id.clone() },
                    Some("dev".into()),
                    &format!("{action} dev"),
                    Some(format!("computer:{id}:{action}")),
                )
                .unwrap(),
        );
    })
}

fn wait_until_queued(
    gate: &'static runtime::operation_gate::OperationGate,
    id: &str,
    count: usize,
) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while gate.waiting_lifecycle_keys(id).len() < count {
        assert!(
            std::time::Instant::now() < deadline,
            "operation never queued"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_queued_start_or_dismiss_error_does_not_cancel_a_running_apply() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    set_approval(&paths, &id, Approval::Auto).unwrap();
    let (guest, entered) = hanging(&id);
    let gate = test_gate();
    let handle = apply_with(gate, guest, &paths, "dev", Trigger::Boot).unwrap();
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    let start = queue_keyed(gate, &id, "start");
    let dismiss = queue_keyed(gate, &id, "dismiss-error");
    wait_until_queued(gate, &id, 2);
    std::thread::sleep(Duration::from_millis(400));
    assert!(
        !handle.is_finished(),
        "a harmless operation cancelled the apply"
    );
    assert!(settings(&paths, &id).last.is_none());
    // A stop still ends it promptly, and the queued work then runs.
    let stop = queue_keyed(gate, &id, "stop");
    handle.join().unwrap();
    for thread in [start, dismiss, stop] {
        thread.join().unwrap();
    }
    assert_eq!(
        settings(&paths, &id).last.unwrap().reason.as_deref(),
        Some("cancelled")
    );
}

#[test]
fn a_queued_start_or_dismiss_error_does_not_cancel_manual_setup() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    let (guest, entered) = hanging(&id);
    let gate = test_gate();
    let handle = manual_setup(gate, guest, paths.clone(), id.clone());
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    let start = queue_keyed(gate, &id, "start");
    let dismiss = queue_keyed(gate, &id, "dismiss-error");
    wait_until_queued(gate, &id, 2);
    std::thread::sleep(Duration::from_millis(400));
    assert!(
        !handle.is_finished(),
        "a harmless operation cancelled setup"
    );
    let restart = queue_keyed(gate, &id, "restart");
    assert!(matches!(
        handle.join().unwrap(),
        Err(RuntimeError::Cancelled { .. })
    ));
    for thread in [start, dismiss, restart] {
        thread.join().unwrap();
    }
}

#[test]
fn an_attempt_whose_marker_cannot_be_saved_does_not_run_the_helper() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    set_approval(&paths, &id, Approval::Auto).unwrap();
    // The settings directory can no longer be written: a file takes its place.
    let settings_directory = directory_of(&paths);
    std::fs::remove_dir_all(&settings_directory).unwrap();
    std::fs::write(&settings_directory, b"not a directory").unwrap();
    let recorder = Recorder::new("{\"apply\":{\"outcome\":\"applied\",\"reason\":null}}\n");
    let result = setup_with(
        test_gate(),
        &recorder,
        &paths,
        &computer_of(&id, true),
        true,
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
    );
    assert!(
        matches!(result, Err(RuntimeError::Unavailable(_))),
        "{result:?}"
    );
    assert!(
        recorder.scripts().is_empty(),
        "the helper ran without a marker"
    );
    // Writable again, the next attempt proceeds and the failure text exists for the panel.
    std::fs::remove_file(&settings_directory).unwrap();
    set_approval(&paths, &id, Approval::Auto).unwrap();
    assert!(begin_attempt(&paths, &id, Approval::Auto).is_ok());
    assert_eq!(read_policy(&paths, &id).unfinished, Some(Approval::Auto));
    assert!(approval_reason_text("state-not-saved").contains("could not save"));
}

#[test]
fn manual_setup_converges_on_a_choice_saved_while_its_helper_runs() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    set_approval(&paths, &id, Approval::Auto).unwrap();
    let guest = Guest::new(&id);
    let gate = test_gate();
    let (entered, receiver) = std::sync::mpsc::channel();
    let release = Arc::new(std::sync::Barrier::new(2));
    *guest.stall.lock().unwrap() = Some((entered, release.clone()));
    let worker = {
        let (guest, paths, id) = (guest.clone(), paths.clone(), id.clone());
        std::thread::spawn(move || {
            let turn = gate.computer(&id, "dev", "Updating dev desktop").unwrap();
            turn.allow_cancel();
            setup_with(
                gate,
                guest.as_ref(),
                &paths,
                &computer_of(&id, true),
                true,
                turn.cancel_token(),
            )
        })
    };
    receiver.recv_timeout(Duration::from_secs(10)).unwrap();
    // Saving the choice is independent of the follow-up worker's admission. That
    // worker can expire while this manual turn runs, so the turn must converge itself.
    set_approval(&paths, &id, Approval::Ask).unwrap();
    release.wait();
    let status = worker.join().unwrap().unwrap();
    assert_eq!(guest.modes(), ["auto", "ask"]);
    assert_eq!(guest.configured().as_deref(), Some("ask"));
    assert_eq!(status["apply"]["approval"], "ask");
    let stored = read_policy(&paths, &id);
    assert_eq!(
        (stored.approval, stored.applied),
        (Approval::Ask, Some(Approval::Ask))
    );
    assert!(!stored.needs_apply());
    assert!(!is_pending(&id));
}

// ------------------------------------------------- network retry

const UNAVAILABLE: Reply = Reply::Report("failed", Some("lcu-archive-unavailable"));
const SHORT: &[Duration] = &[Duration::from_millis(20), Duration::from_millis(20)];
const LONG: &[Duration] = &[Duration::from_secs(3600)];

fn boot_with(
    guest: &Arc<Guest>,
    paths: &RuntimePaths,
    delays: &'static [Duration],
) -> std::thread::JoinHandle<()> {
    apply_with_delays(
        test_gate(),
        guest.clone(),
        paths,
        "dev",
        Trigger::Boot,
        delays,
    )
    .unwrap()
}

fn wait_for(what: &str, condition: impl Fn() -> bool) {
    let end = std::time::Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(
            std::time::Instant::now() < end,
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_network_failure_is_retried_until_the_download_works() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    let guest = Guest::new(&id);
    guest.plan([UNAVAILABLE, UNAVAILABLE]);
    boot_with(&guest, &paths, SHORT).join().unwrap();
    let runs = guest.runs.lock().unwrap().clone();
    // The boot run and two retries (which are not boots).
    assert_eq!(
        runs.iter().map(|run| run.2).collect::<Vec<_>>(),
        [true, false, false]
    );
    let stored = settings(&paths, &id);
    assert_eq!(stored.applied, Some(Approval::Ask));
    assert_eq!(stored.last.unwrap().outcome, Outcome::Applied);
    assert!(!retry_scheduled(&id));
}

#[test]
fn retries_are_bounded_and_the_failure_then_stays() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    let guest = Guest::new(&id);
    guest.plan([UNAVAILABLE, UNAVAILABLE, UNAVAILABLE, UNAVAILABLE]);
    boot_with(&guest, &paths, SHORT).join().unwrap();
    assert_eq!(
        guest.runs.lock().unwrap().len(),
        3,
        "one run and two retries"
    );
    let last = settings(&paths, &id).last.unwrap();
    assert_eq!(
        (last.outcome, last.reason.as_deref()),
        (Outcome::Failed, Some("lcu-archive-unavailable"))
    );
    assert!(!retry_scheduled(&id));
}

#[test]
fn other_failures_are_not_retried() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    let guest = Guest::new(&id);
    guest.plan([
        Reply::Report("failed", Some("lcu-archive-mismatch")),
        UNAVAILABLE,
    ]);
    boot_with(&guest, &paths, SHORT).join().unwrap();
    assert_eq!(guest.runs.lock().unwrap().len(), 1);
}

#[test]
fn a_waiting_retry_is_visible_and_can_be_cancelled() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    let guest = Guest::new(&id);
    guest.plan([UNAVAILABLE]);
    let handle = boot_with(&guest, &paths, LONG);
    wait_for("the retry to be scheduled", || retry_scheduled(&id));
    // While it waits the guest's failure is shown as preparing, with the reason.
    let settings_now = settings(&paths, &id);
    let app = ready();
    let guest_state = guest_status("failed", Some("lcu-archive-unavailable"));
    let inputs = |retrying| Inputs {
        app: Some(&app),
        computer_running: true,
        guest: Some(&guest_state),
        settings: &settings_now,
        pending: false,
        retrying,
    };
    let waiting = state(inputs(retry_scheduled(&id)));
    assert_eq!(waiting["state"], "preparing");
    assert!(waiting["reason"].as_str().unwrap().contains("network"));
    cancel_retry(&id);
    handle.join().unwrap();
    assert_eq!(
        guest.runs.lock().unwrap().len(),
        1,
        "no retry after cancelling"
    );
    assert!(!retry_scheduled(&id));
    // Without a scheduled retry the same guest state is a plain failure.
    let failed = state(inputs(false));
    assert_eq!(failed["state"], "failed");
    assert!(failed["reason"]
        .as_str()
        .unwrap()
        .contains("Silo retries at the next start"));
}

#[test]
fn a_manual_setup_or_a_deletion_takes_over_from_a_waiting_retry() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    let guest = Guest::new(&id);
    guest.plan([UNAVAILABLE]);
    let handle = boot_with(&guest, &paths, LONG);
    wait_for("the retry to be scheduled", || retry_scheduled(&id));
    setup_with(
        test_gate(),
        guest.as_ref(),
        &paths,
        &computer_of(&id, true),
        false,
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
    )
    .unwrap();
    handle.join().unwrap();
    assert!(!retry_scheduled(&id));
    assert_eq!(
        guest.runs.lock().unwrap().len(),
        2,
        "the boot and the manual run only"
    );
    assert_eq!(
        settings(&paths, &id).last.unwrap().outcome,
        Outcome::Applied
    );

    // A deleted computer drops its retry too.
    let guest = Guest::new(&id);
    guest.plan([UNAVAILABLE]);
    let handle = boot_with(&guest, &paths, LONG);
    wait_for("the retry to be scheduled", || retry_scheduled(&id));
    forget(&paths, &id).unwrap();
    handle.join().unwrap();
    assert_eq!(guest.runs.lock().unwrap().len(), 1);
}

#[test]
fn a_retry_ends_when_the_computer_is_no_longer_the_same_running_instance() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(&directory);
    let id = computer();
    write_computers_of(&paths, &id);
    let guest = Guest::new(&id);
    guest.plan([UNAVAILABLE]);
    // The instance is replaced after the checks of the first run (a restart).
    guest.inspects.lock().unwrap().1 = Some(2);
    boot_with(&guest, &paths, SHORT).join().unwrap();
    assert_eq!(guest.runs.lock().unwrap().len(), 1);
    assert!(!retry_scheduled(&id));
}
