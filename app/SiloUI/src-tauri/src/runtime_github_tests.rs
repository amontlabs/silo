//! Explicit hardware integration test. Uses only a disposable managed computer and
//! synthetic or explicitly authorized scoped test credentials. Run with signed
//! SILO_TEST_MSB and SILO_TEST_LIBKRUNFW.
use super::*;

fn restore_live_checkpoint_with_current_profile(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    source: &ComputerConfiguration,
    fork_name: &str,
    profile: &Value,
) -> Result<ComputerConfiguration, String> {
    let checkpoint_id = format!("c{}", &uuid::Uuid::new_v4().simple().to_string()[..31]);
    runner
        .run(
            paths,
            &[
                "snapshot".into(),
                "create".into(),
                checkpoint_id.clone(),
                "--from-sandbox".into(),
                source.name().into(),
                "--full".into(),
                "--guest-flush".into(),
                "required".into(),
                "--integrity".into(),
            ],
            Duration::from_secs(900),
        )
        .map_err(|_| "Could not capture the authenticated checkpoint fixture.".to_string())?;
    let mut fork = source.clone();
    let fork_id = uuid::Uuid::new_v4().to_string();
    fork.id = fork_id.clone();
    fork.name = fork_name.into();
    let mut metadata =
        read_metadata(&paths.metadata).map_err(|_| "Could not prepare the checkpoint fixture.")?;
    metadata.computers.push(fork.clone());
    write_metadata(&paths.metadata, &metadata)
        .map_err(|_| "Could not prepare the checkpoint fixture.")?;
    let observed = inspect_computer(runner, paths, source.name())
        .map_err(|_| "Could not inspect the checkpoint source.")?;
    let policy = observed
        .config
        .pointer("/network/policy")
        .cloned()
        .ok_or("Checkpoint source has no network policy.")?;
    let mut record = checkpoints::Record::default();
    record.pending_checkpoint_restore = Some(checkpoints::PendingRestore {
        checkpoint_id,
        source_computer: source.name().into(),
        state: "full".into(),
    });
    record.desired_network_policy = Some(policy);
    checkpoints::save(paths, &fork_id, &record)
        .map_err(|_| "Could not prepare the checkpoint restore record.")?;

    // The fork's current host assignment is installed before the production
    // restore command runs. The source's write profile is not used for this computer.
    GITHUB_PROFILES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| "GitHub runtime state is unavailable.")?
        .insert((paths.home.clone(), fork_name.into()), profile.to_string());
    checkpoints::start_pending(runner, paths, &fork)
        .map_err(|_| "Production checkpoint restore failed.".to_string())?;
    Ok(fork)
}

fn cleanup_live_checkpoint_fork(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    fork: &ComputerConfiguration,
) {
    let _ = runner.run(
        paths,
        &["stop".into(), fork.name().into()],
        MUTATION_TIMEOUT,
    );
    let _ = runner.run(
        paths,
        &["remove".into(), "--force".into(), fork.name().into()],
        MUTATION_TIMEOUT,
    );
    if let Ok(mut metadata) = read_metadata(&paths.metadata) {
        metadata
            .computers
            .retain(|configuration| configuration.id() != fork.id());
        let _ = write_metadata(&paths.metadata, &metadata);
    }
    let _ = checkpoints::forget_removed(paths, fork.id());
    if let Ok(mut profiles) = GITHUB_PROFILES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        profiles.remove(&(paths.home.clone(), fork.name().into()));
    }
}

#[test]
#[ignore = "requires a signed MicroSandbox binary, hypervisor access, a guest image and GitHub network access"]
fn github_guest_bootstrap_and_live_identity() {
    let _test_state = crate::test_support::global_state();
    crate::test_support::live::require_confirmation();
    let executable = PathBuf::from(std::env::var("SILO_TEST_MSB").expect("set SILO_TEST_MSB"));
    let library =
        PathBuf::from(std::env::var("SILO_TEST_LIBKRUNFW").expect("set SILO_TEST_LIBKRUNFW"));
    let directory = tempfile::Builder::new()
        .prefix("silo-gh-test-")
        .tempdir_in(crate::test_support::live::temp_root())
        .unwrap();
    let paths = RuntimePaths {
        guest_image: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("runtime/guest-image"),
        executable,
        library,
        home: directory.path().join("msb"),
        storage_home: None,
        metadata: directory.path().join("computers.json"),
        volumes: directory.path().join("volumes"),
    };
    let name = "github-integration-test";
    let runner = ProcessRunner;
    let run = |args: &[&str], timeout| {
        runner.run(
            &paths,
            &args.iter().map(|s| (*s).into()).collect::<Vec<_>>(),
            timeout,
        )
    };
    let mut restored_fork: Option<ComputerConfiguration> = None;
    let result = (|| -> Result<(), String> {
        create_disposable_test_computer(&paths, name).map_err(|e| e.to_string())?;
        let initial = inspect_computer(&runner, &paths, name).map_err(|e| e.to_string())?;
        if initial.status != "Stopped" {
            return Err("Bootstrap did not restore stopped state".into());
        }
        let device = device_resources().map_err(|e| e.to_string())?;
        computer_action_with(&runner, &paths, &device, "start", name).map_err(|e| e.to_string())?;
        apply_disposable_test_identity(&paths, name).map_err(|e| e.to_string())?;
        let output = run(
            &[
                "exec",
                name,
                "--user",
                "silo",
                "--no-tty",
                "--quiet",
                "--timeout",
                "30s",
                "--",
                "sh",
                "-c",
                r#"set -eu
 git --version
 git lfs version
 gh --version
 [ "$GH_TOKEN" = '$MSB_SILO_GITHUB' ]
 git var GIT_AUTHOR_IDENT
 printf 'protocol=https\nhost=github.com\n\n' | git credential fill
 "#,
            ],
            MUTATION_TIMEOUT,
        )
        .map_err(|e| e.to_string())?;
        if !output
            .stdout
            .contains("Silo Test <silo-test@example.invalid>")
            || !output.stdout.contains("password=$MSB_SILO_GITHUB")
        {
            return Err("Guest identity or placeholder credential was not verified".into());
        }

        let result = run(
            &[
                "exec",
                name,
                "--user",
                "silo",
                "--no-tty",
                "--quiet",
                "--timeout",
                "30s",
                "--",
                "sh",
                "-c",
                r#"set -eu
 if gh api meta >/tmp/silo-test-response 2>/tmp/silo-test-error; then exit 1; fi
 GH_TOKEN=silo_nonsecret_invalid_probe gh api meta --include >/tmp/silo-test-trust 2>&1 || true
 grep -q 'HTTP/.*401' /tmp/silo-test-trust
 "#,
            ],
            MUTATION_TIMEOUT,
        );
        result.map_err(|e| format!("Disabled access and independent TLS check failed: {e}"))?;
        let profile = serde_json::json!({"version":1,"owners":[{
            "login":"silo-test","readToken":"silo_nonsecret_invalid_probe",
            "writeToken":null,"repositoryIds":[],"expiresAt":4102444800u64
        }]});
        // Exercise the actual app lifecycle argument construction, not a hand-
        // written msb command that could hide credential target wiring errors.
        GITHUB_PROFILES
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap()
            .insert((paths.home.clone(), name.into()), profile.to_string());
        computer_action_with(&runner, &paths, &device, "stop", name).map_err(|e| e.to_string())?;
        for action in ["start", "restart"] {
            computer_action_with(&runner, &paths, &device, action, name)
                .map_err(|e| e.to_string())?;
            let response = run(
                &[
                    "exec",
                    name,
                    "--user",
                    "silo",
                    "--no-tty",
                    "--quiet",
                    "--timeout",
                    "30s",
                    "--",
                    "sh",
                    "-c",
                    "gh api meta --include 2>&1 || true",
                ],
                MUTATION_TIMEOUT,
            )
            .map_err(|e| e.to_string())?
            .stdout;
            if !response.contains("HTTP/") || !response.contains("401") {
                return Err(format!(
                    "Production {action} did not attach the configured GitHub profile"
                ));
            }
        }
        let boot_id = run(
            &[
                "exec",
                name,
                "--user",
                "silo",
                "--no-tty",
                "--quiet",
                "--",
                "cat",
                "/proc/sys/kernel/random/boot_id",
            ],
            READ_TIMEOUT,
        )
        .map_err(|e| e.to_string())?
        .stdout;
        for (profile, should_reach_github) in [
            (profile.to_string(), true),
            (DISABLED_GITHUB_PROFILE.into(), false),
        ] {
            GITHUB_PROFILES
                .get_or_init(|| Mutex::new(HashMap::new()))
                .lock()
                .unwrap()
                .insert((paths.home.clone(), name.into()), profile);
            run(
                &[
                    "modify",
                    name,
                    "--secret",
                    secrets_runtime::SILO_GITHUB_SECRET_SPEC,
                    "--format",
                    "json",
                ],
                MUTATION_TIMEOUT,
            )
            .map_err(|e| e.to_string())?;
            let response = run(
                &[
                    "exec",
                    name,
                    "--user",
                    "silo",
                    "--no-tty",
                    "--quiet",
                    "--timeout",
                    "30s",
                    "--",
                    "sh",
                    "-c",
                    "gh api meta --include 2>&1 || true",
                ],
                MUTATION_TIMEOUT,
            )
            .map_err(|e| e.to_string())?
            .stdout;
            let reached_github = response.contains("HTTP/") && response.contains("401");
            if reached_github != should_reach_github {
                return Err("Live GitHub profile change was not enforced".into());
            }
        }
        let after = run(
            &[
                "exec",
                name,
                "--user",
                "silo",
                "--no-tty",
                "--quiet",
                "--",
                "cat",
                "/proc/sys/kernel/random/boot_id",
            ],
            READ_TIMEOUT,
        )
        .map_err(|e| e.to_string())?
        .stdout;
        if after != boot_id {
            return Err("GitHub profile update restarted the computer".into());
        }
        if inspect_computer(&runner, &paths, name)
            .map_err(|e| e.to_string())?
            .status
            != "Running"
        {
            return Err("Live identity check did not preserve the running computer".into());
        }
        GITHUB_PROFILES
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap()
            .insert((paths.home.clone(), name.into()), profile.to_string());
        run(
            &[
                "modify",
                name,
                "--secret",
                secrets_runtime::SILO_GITHUB_SECRET_SPEC,
                "--format",
                "json",
            ],
            MUTATION_TIMEOUT,
        )
        .map_err(|_| "Could not prepare synthetic checkpoint credentials.")?;
        let source_computer = read_metadata(&paths.metadata)
            .map_err(|_| "Could not inspect synthetic checkpoint source metadata.")?
            .computers
            .into_iter()
            .find(|configuration| configuration.name() == name)
            .ok_or("Synthetic checkpoint source metadata is missing.")?;
        let fork_name = format!(
            "github-restore-{}",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        );
        let fork = restore_live_checkpoint_with_current_profile(
            &runner,
            &paths,
            &source_computer,
            &fork_name,
            &profile,
        )?;
        restored_fork = Some(fork.clone());
        let restored_response = runner
            .run(
                &paths,
                &[
                    "exec".into(),
                    fork_name,
                    "--user".into(),
                    "silo".into(),
                    "--no-tty".into(),
                    "--quiet".into(),
                    "--timeout".into(),
                    "30s".into(),
                    "--".into(),
                    "sh".into(),
                    "-c".into(),
                    "gh api meta --include 2>&1 || true".into(),
                ],
                MUTATION_TIMEOUT,
            )
            .map_err(|_| "Could not verify synthetic checkpoint credentials.")?
            .stdout;
        if !restored_response.contains("HTTP/") || !restored_response.contains("401") {
            return Err("Synthetic checkpoint restore did not use its current profile.".into());
        }
        Ok(())
    })();
    // Cleanup is attempted on every result, including failed creation/provisioning.
    if let Some(fork) = restored_fork.as_ref() {
        cleanup_live_checkpoint_fork(&runner, &paths, fork);
    }
    let stopped = run(&["stop", name], MUTATION_TIMEOUT);
    let removed = run(&["remove", "--force", name], MUTATION_TIMEOUT);
    GITHUB_PROFILES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .remove(&(paths.home.clone(), name.into()));
    assert!(result.is_ok(), "{}", result.unwrap_err());
    assert!(stopped.is_ok(), "Disposable computer could not be stopped");
    assert!(removed.is_ok(), "Disposable computer could not be removed");
}

/// Invoked by github_live_tests with SILO_GITHUB_TEST_VM=1.
/// Credentials are supplied only in the host environment, never test output.
#[test]
#[ignore = "requires explicitly authorized private test repositories and live scoped GitHub credentials"]
fn github_authenticated_guest_workflow() {
    let _test_state = crate::test_support::global_state();
    crate::test_support::live::require_confirmation();
    let required = |key: &str| std::env::var(key).unwrap_or_else(|_| panic!("set {key}"));
    let raw_profile = required("SILO_TEST_GITHUB_PROFILE_JSON");
    let profile: Value = serde_json::from_str(&raw_profile).expect("invalid test profile");
    let read_repo = required("SILO_GITHUB_TEST_READ_REPO");
    let write_repo = required("SILO_GITHUB_TEST_WRITE_REPO");
    let denied_repo = required("SILO_GITHUB_TEST_DENIED_REPO");
    let issue_id = required("SILO_TEST_GITHUB_ISSUE_ID");
    for repo in [&read_repo, &write_repo, &denied_repo] {
        assert!(
            repo.split('/').count() == 2
                && repo.split('/').all(|part| !part.is_empty()
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))),
            "invalid fixture repository"
        );
    }
    assert!(read_repo != write_repo && read_repo != denied_repo && write_repo != denied_repo);
    let secret_values: Vec<String> = profile["owners"]
        .as_array()
        .expect("missing profile owners")
        .iter()
        .flat_map(|owner| [owner["readToken"].as_str(), owner["writeToken"].as_str()])
        .flatten()
        .map(str::to_owned)
        .collect();
    assert!(secret_values.len() >= 2, "missing scoped test credentials");
    let directory = tempfile::Builder::new()
        .prefix("silo-gh-live-")
        .tempdir_in(crate::test_support::live::temp_root())
        .unwrap();
    let paths = RuntimePaths {
        guest_image: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("runtime/guest-image"),
        executable: PathBuf::from(required("SILO_TEST_MSB")),
        library: PathBuf::from(required("SILO_TEST_LIBKRUNFW")),
        home: directory.path().join("msb"),
        storage_home: None,
        metadata: directory.path().join("computers.json"),
        volumes: directory.path().join("volumes"),
    };
    let name = "github-authenticated-test";
    let branch = format!("silo-integration-{}", uuid::Uuid::new_v4().simple());
    let runner = ProcessRunner;
    let run = |args: &[String]| runner.run(&paths, args, MUTATION_TIMEOUT);
    let guest = |script: &str| {
        run(&[
            "exec".into(),
            name.into(),
            "--user".into(),
            "silo".into(),
            "--no-tty".into(),
            "--quiet".into(),
            "--timeout".into(),
            "120s".into(),
            "--".into(),
            "sh".into(),
            "-c".into(),
            script.into(),
            "silo-test".into(),
            read_repo.clone(),
            write_repo.clone(),
            denied_repo.clone(),
            branch.clone(),
            issue_id.clone(),
        ])
    };
    let install = |value: &Value| -> Result<(), String> {
        GITHUB_PROFILES
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap()
            .insert((paths.home.clone(), name.into()), value.to_string());
        run(&[
            "modify".into(),
            name.into(),
            "--secret".into(),
            secrets_runtime::SILO_GITHUB_SECRET_SPEC.into(),
            "--format".into(),
            "json".into(),
        ])
        .map(|_| ())
        .map_err(|_| "Live test credential update failed.".into())
    };
    let mut created = false;
    let mut restored_fork: Option<ComputerConfiguration> = None;
    let result = (|| -> Result<(), String> {
        create_disposable_test_computer(&paths, name)
            .map_err(|_| "Live test computer bootstrap failed.")?;
        created = true;
        GITHUB_PROFILES
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap()
            .insert((paths.home.clone(), name.into()), raw_profile.clone());
        computer_action_with(
            &runner,
            &paths,
            &device_resources().map_err(|_| "Cannot measure device resources.")?,
            "start",
            name,
        )
        .map_err(|_| "Production Start failed for authenticated test computer.")?;
        apply_disposable_test_identity(&paths, name)
            .map_err(|_| "Live test identity setup failed.")?;
        let exposed = guest("env; git config --list --show-origin; printf 'protocol=https\\nhost=github.com\\n\\n' | git credential fill")
            .map_err(|_| "Guest credential boundary check failed.")?.stdout;
        if secret_values.iter().any(|secret| exposed.contains(secret)) {
            return Err("A real credential was exposed inside the guest.".into());
        }
        let boot = guest("cat /proc/sys/kernel/random/boot_id")
            .map_err(|_| "Cannot read test boot ID.")?
            .stdout;
        guest(
            r#"set -eu
mkdir -p /workspace/silo-live
cd /workspace/silo-live
git clone "https://github.com/$1.git" read >/dev/null 2>&1
git clone "https://github.com/$2.git" write >/dev/null 2>&1
if git ls-remote "https://github.com/$3.git" >/dev/null 2>&1; then exit 1; fi
gh api "repos/$1" >/dev/null
gh api graphql -f query='query($owner:String!,$name:String!){repository(owner:$owner,name:$name){nameWithOwner}}' -f owner="${1%/*}" -f name="${1#*/}" --jq '.data.repository.nameWithOwner' | grep -Fx "$1" >/dev/null
gh api graphql -f query='mutation($id:ID!,$title:String!){updateIssue(input:{id:$id,title:$title}){issue{title}}}' -f id="$5" -f title="Silo authenticated guest $4" --jq '.data.updateIssue.issue.title' | grep -Fx "Silo authenticated guest $4" >/dev/null
if gh api "repos/$3" >/dev/null 2>&1; then exit 1; fi
cd read
git checkout -b "$4" >/dev/null 2>&1
git commit --allow-empty -m 'Silo read-only boundary test' >/dev/null
if git push origin "HEAD:refs/heads/$4" >/dev/null 2>&1; then
 git push origin --delete "$4" >/dev/null 2>&1 || true
 exit 1
fi
cd ../write
git checkout -b "$4" >/dev/null 2>&1
git lfs track silo-live.bin >/dev/null
head -c 1048576 /dev/urandom >silo-live.bin
sha256sum silo-live.bin >/workspace/silo-live/expected.sha256
git add .gitattributes silo-live.bin
git commit -m 'Silo isolated Git LFS integration test' >/dev/null
git push origin "HEAD:refs/heads/$4" >/dev/null 2>&1
cd ..
git clone --branch "$4" "https://github.com/$2.git" roundtrip >/dev/null 2>&1
cd roundtrip
sha256sum -c /workspace/silo-live/expected.sha256 >/dev/null
"#,
        )
        .map_err(|_| "Authenticated Git, gh, LFS, or repository boundary test failed.")?;
        let mut readonly = profile.clone();
        for owner in readonly["owners"]
            .as_array_mut()
            .ok_or("Missing test profile owners.")?
        {
            owner["writeToken"] = Value::Null;
        }
        let held_connection = std::env::var("SILO_GITHUB_TEST_INFLIGHT").as_deref() == Ok("1");
        if held_connection {
            guest(r#"set -eu
command -v openssl >/dev/null
probe=/workspace/silo-live/socket-probe
mkdir "$probe"
mkfifo "$probe/input"
( exec 3<>"$probe/input"
  openssl s_client -quiet -ign_eof -verify_return_error -servername api.github.com -connect api.github.com:443 <&3 >"$probe/response" 2>"$probe/tls" || true
  touch "$probe/closed"
) </dev/null >/dev/null 2>&1 &
for expected in 1 2; do
  timeout 5 sh -c 'printf "GET /repos/%s HTTP/1.1\r\nHost: api.github.com\r\nUser-Agent: Silo-revocation-test\r\nAuthorization: Bearer \$MSB_SILO_GITHUB\r\nConnection: keep-alive\r\n\r\n" "$1" >"$2"' sh "$2" "$probe/input"
  ready=0
  for attempt in $(seq 1 50); do
    [ ! -e "$probe/closed" ] || exit 1
    if [ "$(grep -c 'HTTP/1.1 200' "$probe/response" || true)" -ge "$expected" ]; then ready=1; break; fi
    sleep 0.1
  done
  [ "$ready" = 1 ] || exit 1
  sleep 1
  [ ! -e "$probe/closed" ] || exit 1
done
"#).map_err(|_| "Live socket probe inconclusive: authenticated connection did not stay open.")?;
        }
        install(&readonly)?;
        if held_connection {
            guest(r#"set -eu
for attempt in $(seq 1 50); do
  [ ! -e /workspace/silo-live/socket-probe/closed ] || exit 0
  sleep 0.1
done
exit 1
"#).map_err(|_| "Live socket probe failed: policy change did not close held connection within five seconds.")?;
        }
        guest(
            r#"set -eu
cd /workspace/silo-live/write
git fetch origin >/dev/null 2>&1
git commit --allow-empty -m 'Silo live write removal test' >/dev/null
if gh api graphql -f query='mutation($id:ID!){updateIssue(input:{id:$id,title:"unexpected live write"}){issue{id}}}' -f id="$5" >/dev/null 2>&1; then exit 1; fi
gh api graphql -f query='query($id:ID!){node(id:$id){... on Issue{title}}}' -f id="$5" --jq '.data.node.title' | grep -Fx "Silo authenticated guest $4" >/dev/null
if git push origin "HEAD:refs/heads/$4" >/dev/null 2>&1; then exit 1; fi
"#,
        )
        .map_err(|_| "Live write removal was not enforced.")?;
        // Capture a full checkpoint while the source still has its write
        // assignment, then restore a fork whose current host assignment is
        // read-only. Production restore must select the fork profile before
        // starting the checkpoint, without reviving the source's write grant.
        install(&profile)?;
        let source_computer = read_metadata(&paths.metadata)
            .map_err(|_| "Could not inspect the checkpoint source metadata.")?
            .computers
            .into_iter()
            .find(|configuration| configuration.name() == name)
            .ok_or("Checkpoint source metadata is missing.")?;
        let fork_name = format!(
            "github-restore-{}",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        );
        let fork = restore_live_checkpoint_with_current_profile(
            &runner,
            &paths,
            &source_computer,
            &fork_name,
            &readonly,
        )?;
        restored_fork = Some(fork.clone());
        let restored_guest = |script: &str| {
            run(&[
                "exec".into(),
                fork_name.clone(),
                "--user".into(),
                "silo".into(),
                "--no-tty".into(),
                "--quiet".into(),
                "--timeout".into(),
                "120s".into(),
                "--".into(),
                "sh".into(),
                "-c".into(),
                script.into(),
                "silo-test".into(),
                read_repo.clone(),
                write_repo.clone(),
                denied_repo.clone(),
                branch.clone(),
                issue_id.clone(),
            ])
        };
        let restored_env = restored_guest(
            "set -eu; env; git config --list --show-origin; printf 'protocol=https\\nhost=github.com\\n\\n' | git credential fill",
        )
        .map_err(|_| "Could not verify the restored guest credential boundary.")?
        .stdout;
        if secret_values
            .iter()
            .any(|secret| restored_env.contains(secret))
        {
            return Err("A historical credential was exposed in the restored guest.".into());
        }
        restored_guest(
            r#"set -eu
git -C /workspace/silo-live/read ls-remote origin >/dev/null
gh api "repos/$1" >/dev/null
gh api "repos/$2" >/dev/null
cd /workspace/silo-live/write
git fetch origin >/dev/null 2>&1
git checkout -b "$4-restore" >/dev/null 2>&1
git commit --allow-empty -m 'Silo restored read-only policy check' >/dev/null
if git push origin "HEAD:refs/heads/$4-restore" >/dev/null 2>&1; then
 git push origin --delete "$4-restore" >/dev/null 2>&1 || true
 exit 1
fi
if gh api graphql -f query='mutation($id:ID!){updateIssue(input:{id:$id,title:"unexpected restored write"}){issue{id}}}' -f id="$5" >/dev/null 2>&1; then exit 1; fi
"#,
        )
        .map_err(|_| "Restored checkpoint did not use its current read-only assignment.")?;
        // Host Push uses its separately authorized scoped write token while the
        // Computer remains read-only. Exercise the production binary/LFS transfer.
        guest(
            r#"set -eu
cd /workspace/silo-live/write
head -c 1048576 /dev/urandom >silo-live.bin
sha256sum silo-live.bin >/workspace/silo-live/host-expected.sha256
git add silo-live.bin
git commit -m 'Silo isolated host push LFS test' >/dev/null
printf 'uncommitted local data' >uncommitted.txt
printf '#!/bin/sh\nexit 99\n' >.git/hooks/pre-push
chmod +x .git/hooks/pre-push
"#,
        )
        .map_err(|_| "Host Push fixture preparation failed.")?;
        let write_token = profile["owners"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|owner| owner["writeToken"].as_str())
            .ok_or("Missing host push test token.")?;
        let count = crate::host_push::push_committed(
            &paths,
            name,
            "/workspace/silo-live/write",
            &write_repo,
            write_token,
            std::path::Path::new(&required("SILO_TEST_GIT")),
            std::path::Path::new(&required("SILO_TEST_GIT_SUPPORT")),
        )
        .map_err(|_| "Production Host Push failed against private GitHub fixture.")?;
        if count != 2 {
            return Err("Host Push returned an incorrect commit count.".into());
        }
        guest(
            r#"set -eu
cd /workspace/silo-live
git clone --branch "$4" "https://github.com/$2.git" host-roundtrip >/dev/null 2>&1
cd host-roundtrip
sha256sum -c /workspace/silo-live/host-expected.sha256 >/dev/null
[ ! -e uncommitted.txt ]
cd ../write
[ -e uncommitted.txt ]
rm .git/hooks/pre-push
"#,
        )
        .map_err(|_| "Host Push LFS roundtrip or committed-only boundary failed.")?;
        install(&json!({"version":1,"owners":[]}))?;
        guest(
            r#"set -eu
if git ls-remote "https://github.com/$1.git" >/dev/null 2>&1; then exit 1; fi
if gh api "repos/$2" >/dev/null 2>&1; then exit 1; fi
"#,
        )
        .map_err(|_| "Live access disablement was not enforced.")?;
        install(&profile)?;
        guest(
            r#"set -eu
git ls-remote "https://github.com/$1.git" >/dev/null 2>&1
gh api "repos/$2" >/dev/null
"#,
        )
        .map_err(|_| "Live access restoration failed.")?;
        if guest("cat /proc/sys/kernel/random/boot_id")
            .map_err(|_| "Cannot verify boot ID.")?
            .stdout
            != boot
        {
            return Err("Live access changes restarted the test computer.".into());
        }
        Ok(())
    })();
    // Remove only our random branch; never modify the default branch. LFS test
    // objects can remain in GitHub storage after branch deletion, as documented.
    let cleanup = if created {
        install(&profile).and_then(|_| {
            guest(
                r#"set -eu
cleanup_failed=0
for directory in /workspace/silo-live/read /workspace/silo-live/write; do
 [ -d "$directory/.git" ] || continue
 rm -f "$directory/.git/hooks/pre-push"
 for test_branch in "$4" "$4-restore"; do
  if ! remote_branch=$(git -C "$directory" ls-remote origin "refs/heads/$test_branch" 2>/dev/null); then
   cleanup_failed=1
  elif [ -n "$remote_branch" ]; then
   git -C "$directory" push origin --delete "$test_branch" >/dev/null 2>&1 || cleanup_failed=1
  fi
 done
done
exit "$cleanup_failed"
"#,
            )
            .map(|_| ())
            .map_err(|_| "Could not remove live test branch.".into())
        })
    } else {
        Ok(())
    };
    if let Some(fork) = restored_fork.as_ref() {
        cleanup_live_checkpoint_fork(&runner, &paths, fork);
    }
    let _ = run(&["stop".into(), name.into()]);
    let _ = run(&["remove".into(), "--force".into(), name.into()]);
    let absent = run(&["list".into(), "--format".into(), "json".into()])
        .ok()
        .and_then(|output| serde_json::from_str::<Vec<ListedSandbox>>(&output.stdout).ok())
        .is_some_and(|computers| computers.iter().all(|computer| computer.name != name));
    GITHUB_PROFILES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .remove(&(paths.home.clone(), name.into()));
    assert!(
        cleanup.is_ok() && absent,
        "Live test cleanup failed. Inspect both explicit fixture repositories for the unique test branch and the disposable computer. Main workflow passed: {}",
        result.is_ok()
    );
    assert!(result.is_ok(), "{}", result.unwrap_err());
}
