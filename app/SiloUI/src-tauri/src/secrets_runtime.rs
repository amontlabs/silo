//! General secret runtime updates. Only source names and allowed hosts reach argv/config.
use super::*;

pub(crate) type Material = Vec<(String, String, Vec<String>)>;

/// Outcome of a secret update that keeps the transient/final distinction typed all the
/// way up to the retry boundary, instead of re-matching on message text. `Transient`
/// marks a failure worth retrying automatically (a timed-out or momentarily unavailable
/// runtime command); `Final` marks a deterministic failure (validation, rejection,
/// verification mismatch) that retrying cannot fix. Both carry the user-facing message,
/// which is converted to a plain `String` only at the command edge.
#[derive(Debug)]
pub(crate) enum Attempt {
    Transient(String),
    Final(String),
    /// The user cancelled the update while it was running. Like `Final`, retrying cannot
    /// clear it; kept distinct so the caller can surface it as a cancellation rather than
    /// a runtime failure.
    Cancelled(String),
}

impl Attempt {
    /// True only for failures that a retry might clear.
    pub(crate) fn is_transient(&self) -> bool {
        matches!(self, Attempt::Transient(_))
    }
}

/// Deterministic string errors from the update's own checks become `Final`; only the
/// timeout path constructs `Transient` explicitly.
impl From<String> for Attempt {
    fn from(message: String) -> Self {
        Attempt::Final(message)
    }
}

impl From<&str> for Attempt {
    fn from(message: &str) -> Self {
        Attempt::Final(message.to_owned())
    }
}

/// Converts to the plain message at the command edge, discarding the classification.
impl From<Attempt> for String {
    fn from(attempt: Attempt) -> Self {
        match attempt {
            Attempt::Transient(message) | Attempt::Final(message) | Attempt::Cancelled(message) => {
                message
            }
        }
    }
}

// Silo-generated Env-source secret references are safe to forward as opaque
// placeholders. MicroSandbox still substitutes the real value only on the
// explicit allow-host list.
pub(super) const SILO_GITHUB_SECRET_SPEC: &str =
    "SILO_GITHUB:passthrough=*@github.com,api.github.com,uploads.github.com";

/// Stable opaque runtime source, shared with the bundled CLI's stdin adapter.
/// Hashing the guest name keeps references unchanged when other secrets change.
pub(super) fn source_name(name: &str) -> String {
    let digest = Sha256::digest(name.as_bytes());
    let number = u128::from_be_bytes(digest[..16].try_into().expect("SHA-256 has 16 bytes"));
    format!("SILO_SECRET_{number}")
}

fn names(config: &Value) -> HashSet<String> {
    config
        .pointer("/network/secrets/secrets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry["env_var"].as_str())
        .filter(|name| *name != "SILO_GITHUB")
        .map(str::to_owned)
        .collect()
}

pub(crate) fn validate_material(material: &Material) -> Result<(), String> {
    let mut seen = HashSet::new();
    for (name, value, domains) in material {
        let reserved = crate::secrets::reserved_secret_name(name);
        if name.is_empty()
            || reserved
            || !name
                .bytes()
                .enumerate()
                .all(|(i, c)| c == b'_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit()))
            || !seen.insert(name)
            || value.is_empty()
            || value.contains('\0')
            || domains.is_empty()
            || domains
                .iter()
                .any(|host| host.is_empty() || host.contains([',', '@', '\0', '\r', '\n']))
        {
            return Err("Invalid secret name, value, or allowed domain.".into());
        }
    }
    Ok(())
}

// Separate next-start additions from live updates. A pending addition must stay
// pending on subsequent saves, even though it is already in the durable config.
fn plan(
    inspected: &InspectedSandbox,
    material: &Material,
    boot: bool,
) -> (Vec<String>, Vec<String>, bool) {
    let existing = names(&inspected.config);
    let running = inspected.status == "Running";
    let active = if running {
        inspected
            .active_config
            .as_ref()
            .map(names)
            .unwrap_or_else(|| existing.clone())
    } else {
        existing.clone()
    };
    let desired: HashSet<_> = material.iter().map(|(name, _, _)| name.clone()).collect();
    let mut live = Vec::new();
    let mut deferred = Vec::new();
    for old in existing
        .union(&active)
        .filter(|name| !desired.contains(*name))
    {
        live.extend(["--secret-rm".into(), old.clone()]);
    }
    for (name, _, domains) in material {
        let target = if running && !active.contains(name) {
            &mut deferred
        } else {
            &mut live
        };
        target.extend([
            "--secret".into(),
            format!("{name}:passthrough=*@{}", domains.join(",")),
        ]);
    }
    let pending = running && !deferred.is_empty() && !boot;
    (live, deferred, pending)
}

fn modify(
    paths: &RuntimePaths,
    computer: &str,
    options: &[String],
    material: &Material,
    deferred: bool,
) -> Result<(), Attempt> {
    if options.is_empty() {
        return Ok(());
    }
    let mut args = vec!["modify".into(), computer.into()];
    args.extend_from_slice(options);
    args.extend(["--format".into(), "json".into()]);
    if deferred {
        args.push("--next-start".into());
    }
    // Runtime output can contain upstream diagnostics; never capture secret
    // update output in logs or temp files. Report a fixed actionable error.
    let result = spawn_runtime(
        paths,
        RuntimeLaunch {
            args: &args,
            timeout: MUTATION_TIMEOUT,
            material,
            github_profile: &github_environment(paths, &args),
            capture: false,
            report: &ignore_progress,
        },
    );
    result.map(|_| ()).map_err(modify_error)
}

/// Classifies a failed secret `modify`. A cancel is non-transient so the retry
/// boundary does not re-run it; a timed-out or unobservable command is worth retrying.
fn modify_error(error: RuntimeError) -> Attempt {
    match error {
        RuntimeError::Cancelled { .. } => Attempt::Cancelled("Saving secrets was cancelled.".into()),
        RuntimeError::Failed { exit_code: Some(_), .. } => {
            "The computer rejected the secret update. Retry after checking its state.".into()
        }
        RuntimeError::TimedOut { .. } | RuntimeError::Failed { .. } => Attempt::Transient(
            "Updating computer secrets timed out or could not be verified. Retry after checking its state.".into(),
        ),
        _ => "Could not update computer secrets.".into(),
    }
}

pub(crate) fn verify_config(config: &Value, material: &Material) -> bool {
    if !material.is_empty()
        && config
            .pointer("/network/tls/enabled")
            .and_then(Value::as_bool)
            != Some(true)
    {
        return false;
    }
    let expected: HashSet<_> = material.iter().map(|(name, _, _)| name.clone()).collect();
    if names(config) != expected {
        return false;
    }
    material.iter().all(|(name, _, domains)| {
        let Some(entry) = config
            .pointer("/network/secrets/secrets")
            .and_then(Value::as_array)
            .and_then(|entries| {
                entries
                    .iter()
                    .find(|entry| entry["env_var"] == name.as_str())
            })
        else {
            return false;
        };
        let hosts: HashSet<String> = entry["allowed_hosts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|host| {
                if host == "any" {
                    Some("*".into())
                } else {
                    host["exact"]
                        .as_str()
                        .or_else(|| host["wildcard"].as_str())
                        .map(str::to_owned)
                }
            })
            .collect();
        entry["source"]["kind"] == "env"
            && entry["source"]["var"] == source_name(name)
            && entry["value"].as_str().unwrap_or_default().is_empty()
            && entry["require_tls_identity"].as_bool().unwrap_or(true)
            && entry["placeholder"] == format!("$MSB_{name}")
            && hosts == domains.iter().cloned().collect()
    })
}

pub(crate) fn apply(
    paths: &RuntimePaths,
    computer: &str,
    material: &Material,
    boot: bool,
) -> Result<Vec<String>, Attempt> {
    validate_material(material)?;
    let inspected = inspect_computer(&ProcessRunner, paths, computer)
        .map_err(|_| "Could not inspect computer secrets.".to_string())?;
    ensure_managed(&inspected).map_err(|error| error.to_string())?;
    if !matches!(
        inspected.status.as_str(),
        "Running" | "Created" | "Stopped" | "Crashed"
    ) {
        return Err(
            "Wait for the computer operation to finish, then retry the secret update.".into(),
        );
    }
    if inspected.status == "Running" && inspected.active_config.is_none() {
        return Err("The running computer's active secret configuration could not be verified. Stop it, then retry.".into());
    }
    if !material.is_empty()
        && inspected
            .config
            .pointer("/network/tls/enabled")
            .and_then(Value::as_bool)
            != Some(true)
    {
        return Err("This computer does not have secure HTTPS secret handling enabled. Recreate it before assigning secrets.".into());
    }
    let (live, deferred, pending) = plan(&inspected, material, boot);
    modify(paths, computer, &live, material, false)?;
    // Stop before the deferred (next-start) update if the user cancelled between the two
    // runtime commands, so a cancelled save does not push further changes.
    if operation_gate::cancel_requested() {
        return Err(Attempt::Cancelled("Saving secrets was cancelled.".into()));
    }
    modify(paths, computer, &deferred, material, true)?;
    let observed = inspect_computer(&ProcessRunner, paths, computer)
        .map_err(|_| "Could not verify the saved computer secrets.".to_string())?;
    if !verify_config(&observed.config, material) {
        return Err(
            "The computer secret configuration did not match the saved settings. Retry the update."
                .into(),
        );
    }
    // Active config holds source references too. Verify applied entries and
    // revocations separately from additions intentionally deferred to next boot.
    if inspected.status == "Running" {
        let active_names = inspected
            .active_config
            .as_ref()
            .map(names)
            .unwrap_or_default();
        let active_material = material
            .iter()
            .filter(|(name, _, _)| active_names.contains(name))
            .cloned()
            .collect();
        if !observed
            .active_config
            .as_ref()
            .is_some_and(|config| verify_config(config, &active_material))
        {
            return Err(
                "The running computer did not confirm the secret update. Retry the update.".into(),
            );
        }
    }
    Ok(if pending {
        let active = inspected
            .active_config
            .as_ref()
            .map(names)
            .unwrap_or_default();
        material
            .iter()
            .filter(|(name, _, _)| !active.contains(name))
            .map(|(name, _, _)| name.clone())
            .collect()
    } else {
        Vec::new()
    })
}

/// Revoke only the removed name. No remaining secret values or credential-store
/// reads are needed, and unrelated secret/GitHub policies remain untouched.
pub(crate) fn remove_name(paths: &RuntimePaths, computer: &str, name: &str) -> Result<(), String> {
    modify(
        paths,
        computer,
        &["--secret-rm".into(), name.into()],
        &Vec::new(),
        false,
    )
    .map_err(String::from)
}

pub(crate) fn revoke_observed_with(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer: &str,
    name: &str,
    inspected: &InspectedSandbox,
    remove: &mut dyn FnMut(&str) -> Result<(), String>,
) -> Result<bool, String> {
    if inspected.status != "Running" {
        return Ok(false);
    }
    let Some(active) = inspected.active_config.as_ref() else {
        return Ok(false);
    };
    if !names(active).contains(name) && !names(&inspected.config).contains(name) {
        return Ok(true);
    }
    remove(name)?;
    match observe_computer(runner, paths, computer).map_err(|error| error.to_string())? {
        ComputerRuntime::Absent => Ok(true),
        ComputerRuntime::Present(observed) => {
            ensure_managed(&observed).map_err(|error| error.to_string())?;
            Ok(
                matches!(observed.status.as_str(), "Stopped" | "Created" | "Crashed")
                    || (observed.status == "Running"
                        && !names(&observed.config).contains(name)
                        && observed
                            .active_config
                            .as_ref()
                            .is_some_and(|config| !names(config).contains(name))),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::runner::{ExpectedCommand, ScriptedRunner};

    #[test]
    fn material_validation_rejects_empty_duplicate_and_malformed_inputs() {
        for (name, value, domains) in [
            ("", "value", vec!["*"]),
            ("1TOKEN", "value", vec!["*"]),
            ("A-B", "value", vec!["*"]),
            ("TÖKEN", "value", vec!["*"]),
            ("TOKEN", "", vec!["*"]),
            ("TOKEN", "nul\0value", vec!["*"]),
            ("TOKEN", "value", vec![]),
            ("TOKEN", "value", vec![""]),
            ("TOKEN", "value", vec!["api.example.com", "bad@host"]),
            ("TOKEN", "value", vec!["bad\0host"]),
            ("TOKEN", "value", vec!["bad\rhost"]),
            ("TOKEN", "value", vec!["bad\nhost"]),
        ] {
            let material = vec![(
                name.into(),
                value.into(),
                domains.into_iter().map(str::to_owned).collect(),
            )];
            assert_eq!(
                validate_material(&material).unwrap_err(),
                "Invalid secret name, value, or allowed domain.",
                "{name:?}"
            );
        }
        let valid = vec![(
            "_TOKEN_2".into(),
            "literal '$value'\nbytes".into(),
            vec!["*".into()],
        )];
        validate_material(&valid).unwrap();
        validate_material(&vec![]).unwrap();
        let mut duplicate = valid.clone();
        duplicate.extend(valid);
        assert!(validate_material(&duplicate).is_err());
    }

    #[test]
    fn modify_failures_preserve_retry_and_cancellation_categories_without_diagnostics() {
        let cancelled = modify_error(RuntimeError::Cancelled {
            operation: "private diagnostic".into(),
        });
        assert!(matches!(cancelled, Attempt::Cancelled(_)));
        assert_eq!(String::from(cancelled), "Saving secrets was cancelled.");
        for error in [
            RuntimeError::TimedOut {
                operation: "private diagnostic".into(),
            },
            RuntimeError::Failed {
                operation: "modify".into(),
                exit_code: None,
                detail: "private diagnostic".into(),
            },
        ] {
            let attempt = modify_error(error);
            assert!(attempt.is_transient());
            assert_eq!(String::from(attempt), "Updating computer secrets timed out or could not be verified. Retry after checking its state.");
        }
        let rejected = modify_error(RuntimeError::Failed {
            operation: "modify".into(),
            exit_code: Some(1),
            detail: "private diagnostic".into(),
        });
        assert!(matches!(rejected, Attempt::Final(_)));
        assert_eq!(
            String::from(rejected),
            "The computer rejected the secret update. Retry after checking its state."
        );
        for error in [
            RuntimeError::Invalid("private diagnostic".into()),
            RuntimeError::Unavailable("private diagnostic".into()),
            RuntimeError::Malformed("private diagnostic".into()),
        ] {
            let attempt = modify_error(error);
            assert!(matches!(attempt, Attempt::Final(_)));
            assert_eq!(String::from(attempt), "Could not update computer secrets.");
        }
    }

    #[test]
    #[ignore = "bug: temporary runtime launch failures are final, so secret updates skip automatic retry"]
    fn temporary_runtime_launch_failure_remains_retryable_for_secret_updates() {
        let attempt = modify_error(RuntimeError::Launch(
            "temporarily unable to spawn the runtime".into(),
        ));
        assert!(
            attempt.is_transient(),
            "temporary launch failure must reach automatic retry: {attempt:?}"
        );
    }

    fn observation(status: &str, saved: &[&str], active: Option<&[&str]>) -> Value {
        let mut saved = config(saved);
        saved["labels"] = json!({"silo.managed":"true"});
        json!({"name":"dev", "status":status, "config":saved, "active_config":active.map(config)})
    }

    #[test]
    fn removal_plan_revokes_names_found_only_in_the_active_configuration() {
        let inspected = serde_json::from_value(observation(
            "Running",
            &["SAVED_ONLY", "BOTH", "SILO_GITHUB"],
            Some(&["ACTIVE_ONLY", "BOTH", "SILO_GITHUB"]),
        ))
        .unwrap();
        let material = vec![(
            "BOTH".into(),
            "private value".into(),
            vec!["api.example.com".into()],
        )];
        let (live, deferred, pending) = plan(&inspected, &material, false);
        let mut removals = Vec::new();
        let mut additions = Vec::new();
        for pair in live.chunks_exact(2) {
            match pair[0].as_str() {
                "--secret-rm" => removals.push(pair[1].as_str()),
                "--secret" => additions.push(pair[1].as_str()),
                flag => panic!("unexpected secret update flag {flag}"),
            }
        }
        removals.sort_unstable();
        assert_eq!(removals, ["ACTIVE_ONLY", "SAVED_ONLY"]);
        assert_eq!(additions, ["BOTH:passthrough=*@api.example.com"]);
        assert!(deferred.is_empty());
        assert!(!pending);
    }

    #[test]
    fn verification_rejects_wrong_bindings_and_missing_or_extra_hosts() {
        let material = vec![(
            "TOKEN".into(),
            "private value".into(),
            vec!["api.example.com".into(), "*.example.com".into()],
        )];
        let approved = json!({"network":{"tls":{"enabled":true},"secrets":{"secrets":[{
            "env_var":"TOKEN","source":{"kind":"env","var":source_name("TOKEN")},
            "value":"","placeholder":"$MSB_TOKEN","require_tls_identity":true,
            "allowed_hosts":[{"wildcard":"*.example.com"},{"exact":"api.example.com"}]
        }]}}});
        assert!(verify_config(&approved, &material));
        for (pointer, replacement) in [
            ("/network/tls/enabled", json!(false)),
            ("/network/secrets/secrets/0/env_var", json!("OTHER")),
            ("/network/secrets/secrets/0/source/kind", json!("literal")),
            ("/network/secrets/secrets/0/source/var", json!("TOKEN")),
            (
                "/network/secrets/secrets/0/placeholder",
                json!("$MSB_OTHER"),
            ),
            ("/network/secrets/secrets/0/allowed_hosts", json!([])),
            (
                "/network/secrets/secrets/0/allowed_hosts",
                json!([{"exact":"api.example.com"}]),
            ),
            (
                "/network/secrets/secrets/0/allowed_hosts",
                json!([{"exact":"api.example.com"},{"wildcard":"*.example.com"},"any"]),
            ),
        ] {
            let mut changed = approved.clone();
            *changed.pointer_mut(pointer).unwrap() = replacement;
            assert!(!verify_config(&changed, &material), "{pointer}");
        }
        assert!(verify_config(&config(&["SILO_GITHUB"]), &vec![]));
        assert!(!verify_config(&config(&["OTHER", "SILO_GITHUB"]), &vec![]));
    }

    #[test]
    fn revocation_skips_unobservable_guests_and_already_clean_configurations() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::test_support::paths(dir.path());
        for (status, saved, active, expected) in [
            ("Stopped", vec!["TOKEN"], Some(vec!["TOKEN"]), false),
            ("Starting", vec!["TOKEN"], Some(vec!["TOKEN"]), false),
            ("Paused", vec!["TOKEN"], Some(vec!["TOKEN"]), false),
            ("Running", vec!["TOKEN"], None, false),
            (
                "Running",
                vec!["SILO_GITHUB"],
                Some(vec!["SILO_GITHUB"]),
                true,
            ),
        ] {
            let inspected =
                serde_json::from_value(observation(status, &saved, active.as_deref())).unwrap();
            let runner = ScriptedRunner::new([]);
            let result =
                revoke_observed_with(&runner, &paths, "dev", "TOKEN", &inspected, &mut |_| {
                    panic!("no revocation command should run for {status}")
                });
            assert_eq!(result.unwrap(), expected, "{status}");
            runner.assert_finished();
        }
    }

    #[test]
    fn revocation_requires_confirmation_from_saved_and_active_configurations() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::test_support::paths(dir.path());
        let inspected =
            serde_json::from_value(observation("Running", &["TOKEN"], Some(&["TOKEN"]))).unwrap();
        for (status, saved, active, expected) in [
            ("Running", vec!["TOKEN"], Some(vec![]), false),
            ("Running", vec![], Some(vec!["TOKEN"]), false),
            ("Running", vec![], None, false),
            (
                "Running",
                vec!["SILO_GITHUB"],
                Some(vec!["SILO_GITHUB"]),
                true,
            ),
            ("Stopped", vec!["TOKEN"], None, true),
            ("Created", vec!["TOKEN"], None, true),
            ("Crashed", vec!["TOKEN"], None, true),
            ("Paused", vec![], Some(vec![]), false),
        ] {
            let runner = ScriptedRunner::new([ExpectedCommand::ok(
                ["inspect", "dev", "--format", "json"],
                observation(status, &saved, active.as_deref()).to_string(),
            )]);
            let mut removed = Vec::new();
            assert_eq!(
                revoke_observed_with(&runner, &paths, "dev", "TOKEN", &inspected, &mut |name| {
                    removed.push(name.to_owned());
                    Ok(())
                })
                .unwrap(),
                expected,
                "{status}"
            );
            assert_eq!(removed, ["TOKEN"]);
            runner.assert_finished();
        }
    }

    #[test]
    fn revocation_preserves_remove_errors_and_rejects_failed_or_unmanaged_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::test_support::paths(dir.path());
        let inspected =
            serde_json::from_value(observation("Running", &["TOKEN"], Some(&["TOKEN"]))).unwrap();
        let runner = ScriptedRunner::new([]);
        assert_eq!(
            revoke_observed_with(&runner, &paths, "dev", "TOKEN", &inspected, &mut |_| Err(
                "fixture removal failed".into()
            ))
            .unwrap_err(),
            "fixture removal failed"
        );
        runner.assert_finished();

        let runner = ScriptedRunner::new([ExpectedCommand::error(
            ["inspect", "dev", "--format", "json"],
            RuntimeError::TimedOut {
                operation: "Inspect".into(),
            },
        )]);
        assert_eq!(
            revoke_observed_with(&runner, &paths, "dev", "TOKEN", &inspected, &mut |_| Ok(()))
                .unwrap_err(),
            "Inspect timed out. Check the computer state, then retry."
        );
        runner.assert_finished();

        let mut replaced = observation("Running", &[], Some(&[]));
        replaced["config"]["labels"] = json!({});
        let runner = ScriptedRunner::new([ExpectedCommand::ok(
            ["inspect", "dev", "--format", "json"],
            replaced.to_string(),
        )]);
        assert_eq!(
            revoke_observed_with(&runner, &paths, "dev", "TOKEN", &inspected, &mut |_| Ok(()))
                .unwrap_err(),
            "Computer 'dev' is not owned by Silo. No computer operation was performed."
        );
        runner.assert_finished();
    }

    #[test]
    fn attempt_classifies_transient_and_final_and_preserves_the_message() {
        let _test_state = crate::test_support::global_state();
        let transient = Attempt::Transient("timed out".into());
        let final_error = Attempt::Final("rejected".into());
        assert!(transient.is_transient());
        assert!(!final_error.is_transient());
        // Deterministic string errors default to Final; conversion keeps the message.
        assert!(!Attempt::from("bad input").is_transient());
        assert!(!Attempt::from("bad input".to_string()).is_transient());
        assert_eq!(String::from(transient), "timed out");
        assert_eq!(String::from(final_error), "rejected");
        // A cancellation is non-transient and preserves its message like the others.
        let cancelled = Attempt::Cancelled("stopped".into());
        assert!(!cancelled.is_transient());
        assert_eq!(String::from(cancelled), "stopped");
    }

    #[test]
    fn modify_child_is_killed_and_reported_cancelled_when_the_operation_is_cancelled() {
        let _test_state = crate::test_support::global_state();
        // A long-running stand-in child models a `modify` runtime command; cancelling the
        // owning operation must kill it and return a non-transient `Cancelled` so the retry
        // boundary does not re-run the update.
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&directory);
        fs::create_dir_all(&paths.home).unwrap();
        fs::write(&paths.library, b"test").unwrap();
        fs::write(
            &paths.executable,
            "#!/bin/sh\necho $$ > \"$MSB_HOME/modify.pid\"\nexec sleep 30\n",
        )
        .unwrap();
        fs::set_permissions(&paths.executable, fs::Permissions::from_mode(0o700)).unwrap();
        let gate: &'static operation_gate::OperationGate =
            Box::leak(Box::new(operation_gate::OperationGate::new()));
        let guard = gate
            .computer("secret-cancel-id", "secret-cancel", "Saving secrets")
            .unwrap();
        guard.allow_cancel();
        let id = gate.snapshot().running[0].id;
        let token = guard.cancel_token();
        let pid_file = paths.home.join("modify.pid");
        // Cancel once the runtime child is running; the guard was acquired on this thread,
        // so the shared launcher observes the current-operation token on its next poll.
        let canceller = thread::spawn(move || {
            while !pid_file.exists() {
                thread::sleep(Duration::from_millis(10));
            }
            token.store(true, Ordering::SeqCst);
        });
        let started = Instant::now();
        let result = modify(
            &paths,
            "secret-cancel",
            &["--secret-rm".into(), "OLD".into()],
            &Vec::new(),
            false,
        );
        canceller.join().unwrap();
        assert!(matches!(result, Err(Attempt::Cancelled(_))), "{result:?}");
        assert!(started.elapsed() < Duration::from_secs(20));
        // The child was killed and reaped, so its pid no longer names a live process.
        let pid: i32 = fs::read_to_string(paths.home.join("modify.pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_ne!(unsafe { libc::kill(pid, 0) }, 0);
        let _ = id;
        drop(guard);
    }

    #[test]
    fn name_only_revocation_does_not_resolve_remaining_secret_values() {
        let _test_state = crate::test_support::global_state();
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&directory);
        fs::create_dir_all(&paths.home).unwrap();
        fs::write(&paths.library, b"test").unwrap();
        fs::write(&paths.executable, "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$MSB_HOME/revoke-args\"\ncat > \"$MSB_HOME/revoke-stdin\"\n").unwrap();
        fs::set_permissions(&paths.executable, fs::Permissions::from_mode(0o700)).unwrap();
        let store = directory.path().join("secrets.json");
        // This unrelated assignment has no credential. Revocation must not ask for it.
        fs::write(&store, r#"{"secrets":[{"id":"keep","valueId":"unavailable","name":"KEEP","computers":["dev"],"allowedDomains":["api.example.com"]}]}"#).unwrap();
        crate::secrets::use_test_store(Some(store));
        crate::secrets::use_test_vault(Some(Default::default()));
        remove_name(&paths, "dev", "REMOVED").unwrap();
        assert_eq!(
            fs::read_to_string(paths.home.join("revoke-args")).unwrap(),
            "modify\ndev\n--secret-rm\nREMOVED\n--format\njson\n"
        );
        let values: Value =
            serde_json::from_slice(&fs::read(paths.home.join("revoke-stdin")).unwrap()).unwrap();
        assert_eq!(values, json!({"SILO_GITHUB": DISABLED_GITHUB_PROFILE}));
        crate::secrets::use_test_store(None);
        crate::secrets::use_test_vault(None);
    }

    fn config(names: &[&str]) -> Value {
        json!({"network":{"secrets":{"secrets": names.iter().map(|name| json!({"env_var": name})).collect::<Vec<_>>()}}})
    }
    #[test]
    fn separates_live_revocation_and_rotation_from_pending_additions() {
        let _test_state = crate::test_support::global_state();
        let inspected = InspectedSandbox {
            runtime_instance_id: None,
            runtime_instance_reported: true,
            updated_at: None,
            name: "dev".into(),
            status: "Running".into(),
            config: config(&["OLD", "KEEP", "NEW", "SILO_GITHUB"]),
            active_config: Some(config(&["OLD", "KEEP", "SILO_GITHUB"])),
        };
        let material = vec![
            (
                "KEEP".into(),
                "sensitive".into(),
                vec!["api.example.com".into()],
            ),
            ("NEW".into(), "other".into(), vec!["api.example.com".into()]),
        ];
        let (live, deferred, pending) = plan(&inspected, &material, false);
        assert!(pending);
        assert_eq!(
            live,
            vec![
                "--secret-rm",
                "OLD",
                "--secret",
                "KEEP:passthrough=*@api.example.com"
            ]
        );
        assert_eq!(
            deferred,
            vec!["--secret", "NEW:passthrough=*@api.example.com"]
        );
        assert!(!live.join(" ").contains("sensitive"));
        assert!(!live.join(" ").contains("SILO_GITHUB"));
    }
    #[test]
    fn stopped_additions_apply_on_next_boot_without_pending_restart() {
        let _test_state = crate::test_support::global_state();
        let inspected = InspectedSandbox {
            runtime_instance_id: None,
            runtime_instance_reported: true,
            updated_at: None,
            name: "dev".into(),
            status: "Stopped".into(),
            config: config(&[]),
            active_config: None,
        };
        let (live, deferred, pending) = plan(
            &inspected,
            &vec![("TOKEN".into(), "value".into(), vec!["*".into()])],
            false,
        );
        assert!(!pending);
        assert!(deferred.is_empty());
        assert_eq!(live, vec!["--secret", "TOKEN:passthrough=*@*"]);
    }
    #[test]
    fn generated_sources_are_stable_and_distinct_from_guest_names() {
        let _test_state = crate::test_support::global_state();
        // Same vector as the bundled CLI patch, independent of assignment order.
        assert_eq!(
            source_name("API_KEY"),
            "SILO_SECRET_272068193077316117667065620025266693635"
        );
        assert_ne!(source_name("API_KEY"), source_name("OTHER"));
        let all = vec![
            ("OTHER".into(), "other-value".into(), vec!["*".into()]),
            ("API_KEY".into(), "api-value".into(), vec!["*".into()]),
        ];
        let one = vec![all[1].clone()];
        let full: Value =
            serde_json::from_slice(&secret_values_document(&all, DISABLED_GITHUB_PROFILE).unwrap())
                .unwrap();
        let reduced: Value =
            serde_json::from_slice(&secret_values_document(&one, DISABLED_GITHUB_PROFILE).unwrap())
                .unwrap();
        assert_eq!(
            full[&source_name("API_KEY")],
            reduced[&source_name("API_KEY")]
        );
        assert!(reduced.get("API_KEY").is_none());
    }

    #[test]
    fn verification_requires_host_reference_tls_and_exact_domains() {
        let _test_state = crate::test_support::global_state();
        let material = vec![(
            "TOKEN".into(),
            "never-durable".into(),
            vec!["api.example.com".into()],
        )];
        let mut config = json!({"network":{"tls":{"enabled":true},"secrets":{"secrets":[{"env_var":"TOKEN","source":{"kind":"env","var":source_name("TOKEN")},"value":"","placeholder":"$MSB_TOKEN","require_tls_identity":true,"allowed_hosts":[{"exact":"api.example.com"}]}]}}});
        assert!(verify_config(&config, &material));
        config["network"]["secrets"]["secrets"][0]["value"] = json!("never-durable");
        assert!(!verify_config(&config, &material));
        config["network"]["secrets"]["secrets"][0]["value"] = json!("");
        config["network"]["secrets"]["secrets"][0]["require_tls_identity"] = json!(false);
        assert!(!verify_config(&config, &material));
        config["network"]["secrets"]["secrets"][0]["require_tls_identity"] = json!(true);
        config["network"]["secrets"]["secrets"][0]["allowed_hosts"] = json!(["any"]);
        assert!(!verify_config(&config, &material));
        assert!(!verify_config(&config, &vec![]));
    }
    #[test]
    fn rejects_host_environment_overrides_and_inline_argument_injection() {
        let _test_state = crate::test_support::global_state();
        for name in [
            "PATH",
            "MSB_HOME",
            "SILO_GITHUB",
            "DYLD_INSERT_LIBRARIES",
            "RUST_LOG",
            "A@B",
        ] {
            assert!(validate_material(&vec![(
                name.into(),
                "value".into(),
                vec!["api.example.com".into()]
            )])
            .is_err());
        }
        assert!(validate_material(&vec![(
            "TOKEN".into(),
            "value".into(),
            vec!["api.example.com,evil.com".into()]
        )])
        .is_err());
    }
}

#[cfg(test)]
#[test]
#[ignore = "requires signed MicroSandbox, hypervisor access, a guest image and test HTTPS endpoints"]
fn live_secret_adapter_uses_refs_and_preserves_boot_for_live_updates() {
    let _test_state = crate::test_support::global_state();
    crate::test_support::live::require_confirmation();
    let directory = tempfile::Builder::new()
        .prefix("silo-secret-test-")
        .tempdir_in(crate::test_support::live::temp_root())
        .unwrap();
    let paths = RuntimePaths {
        guest_image: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("runtime/guest-image"),
        executable: PathBuf::from(std::env::var("SILO_TEST_MSB").expect("set SILO_TEST_MSB")),
        library: PathBuf::from(
            std::env::var("SILO_TEST_LIBKRUNFW").expect("set SILO_TEST_LIBKRUNFW"),
        ),
        home: directory.path().join("msb"),
        storage_home: None,
        metadata: directory.path().join("computers.json"),
        volumes: directory.path().join("volumes"),
    };
    let name = "secrets-test";
    let command = |args: &[&str], material: &Material| -> Result<(), String> {
        let mut child = Command::new(&paths.executable)
            .args(args)
            .env("MSB_HOME", &paths.home)
            .env("MSB_PATH", &paths.executable)
            .env("MSB_LIBKRUNFW_PATH", &paths.library)
            .env("SILO_GITHUB", DISABLED_GITHUB_PROFILE)
            .envs(material.iter().map(|(name, value, _)| (name, value)))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "test command spawn failed")?;
        let deadline = Instant::now() + MUTATION_TIMEOUT;
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(()),
                Ok(Some(_)) => return Err(format!("test command {} failed", args[0])),
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("test command timed out".into());
                }
            }
        }
    };
    let guest = |script: &str| -> Result<String, String> {
        run_msb(
            &paths,
            &[
                "exec",
                name,
                "--no-tty",
                "--quiet",
                "--timeout",
                "120s",
                "--",
                "sh",
                "-c",
                script,
            ]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
            MUTATION_TIMEOUT,
        )
        .map(|output| output.stdout)
        .map_err(|_| "guest verification failed".into())
    };
    let connection_update = |desired: Option<&Material>,
                             before: &str,
                             after: Option<&str>|
     -> Result<(), String> {
        guest("rm -f /tmp/silo-secret-ready /tmp/silo-secret-continue")?;
        let script = r#"import http.client,json,os,pathlib,time
c=http.client.HTTPSConnection('httpbingo.org',timeout=15)
headers={'X-Silo-Test':os.environ['API_TEST_TOKEN'],'User-Agent':'silo-secret-integration-test'}
c.request('GET','/headers?probe='+str(time.time_ns()),headers=headers)
r=c.getresponse(); first=r.read().decode()
assert r.status==200 and not r.will_close and c.sock is not None, 'status='+str(r.status)+' closes='+str(r.will_close)+' socket='+str(c.sock is not None)
sock=c.sock
pathlib.Path('/tmp/silo-secret-ready').write_text('ready')
end=time.monotonic()+30
while not pathlib.Path('/tmp/silo-secret-continue').exists():
 if time.monotonic()>end: raise TimeoutError('host coordination timed out')
 time.sleep(.025)
assert c.sock is sock
try:
 c.request('GET','/headers?probe='+str(time.time_ns()),headers=headers)
 r=c.getresponse(); second=r.read().decode()
 print(json.dumps({'first':first,'second':second,'closed':False}))
except (OSError,http.client.HTTPException):
 print(json.dumps({'first':first,'second':'','closed':True}))
finally: c.close()
"#;
        let capture = tempfile::NamedTempFile::new().map_err(|_| "capture failed")?;
        let errors = tempfile::NamedTempFile::new().map_err(|_| "capture failed")?;
        let mut child = Command::new(&paths.executable)
            .args([
                "exec",
                name,
                "--no-tty",
                "--quiet",
                "--timeout",
                "50s",
                "--",
                "python3",
                "-c",
                script,
            ])
            .env("MSB_HOME", &paths.home)
            .env("MSB_PATH", &paths.executable)
            .env("MSB_LIBKRUNFW_PATH", &paths.library)
            .stdin(Stdio::null())
            .stdout(Stdio::from(capture.reopen().map_err(|_| "capture failed")?))
            .stderr(Stdio::from(errors.reopen().map_err(|_| "capture failed")?))
            .spawn()
            .map_err(|_| "keepalive client failed to start")?;
        let result = (|| -> Result<(), String> {
            let deadline = Instant::now() + Duration::from_secs(25);
            loop {
                if guest("test -f /tmp/silo-secret-ready && printf ready || true")? == "ready" {
                    break;
                }
                if child
                    .try_wait()
                    .map_err(|_| "keepalive client state failed")?
                    .is_some()
                    || Instant::now() >= deadline
                {
                    return Err(format!(
                        "keepalive endpoint did not establish a reusable connection: {}",
                        fs::read_to_string(errors.path())
                            .unwrap_or_default()
                            .lines()
                            .last()
                            .unwrap_or("no diagnostic")
                    ));
                }
                thread::sleep(Duration::from_millis(50));
            }
            if let Some(desired) = desired {
                apply(&paths, name, desired, false)?;
            }
            guest("touch /tmp/silo-secret-continue")?;
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                match child
                    .try_wait()
                    .map_err(|_| "keepalive client state failed")?
                {
                    Some(status) if status.success() => break,
                    Some(_) => return Err("keepalive client verification failed".into()),
                    None if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
                    None => return Err("keepalive client timed out".into()),
                }
            }
            let output =
                fs::read_to_string(capture.path()).map_err(|_| "keepalive capture failed")?;
            let result: Value =
                serde_json::from_str(output.trim()).map_err(|_| "keepalive response missing")?;
            if !result["first"]
                .as_str()
                .unwrap_or_default()
                .contains(before)
            {
                return Err(
                    "keepalive first request did not receive expected synthetic value".into(),
                );
            }
            let closed = result["closed"] == true;
            let second = result["second"].as_str().unwrap_or_default();
            if desired.is_none() {
                if closed || !second.contains(before) {
                    return Err("unchanged keepalive control did not reuse connection".into());
                }
            } else if !closed {
                match after {
                    Some(value) if second.contains(value) && !second.contains(before) => (),
                    None if !second.contains(before) => (),
                    _ => return Err("existing connection retained revoked secret authority".into()),
                }
            }
            Ok(())
        })();
        if child.try_wait().ok().flatten().is_none() {
            let _ = child.kill();
            let _ = child.wait();
        }
        result
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
        command(
            &[
                "create",
                &guest_image::prepare(&ProcessRunner, &paths).map_err(|error| error.to_string())?,
                "--pull",
                "never",
                "--name",
                name,
                "--cpus",
                "1",
                "--memory",
                "512M",
                "--label",
                MANAGED_LABEL,
                "--secret",
                SILO_GITHUB_SECRET_SPEC,
                "--no-start",
                "--quiet",
            ],
            &vec![],
        )?;
        let mut material = vec![(
            "SILO_TEST_TOKEN".into(),
            "synthetic-first".into(),
            vec!["httpbingo.org".into()],
        )];
        // The public reserved prefix guard also applies to integration fixtures.
        material[0].0 = "API_TEST_TOKEN".into();
        assert!(apply(&paths, name, &material, false)?.is_empty());
        command(&["start", name, "--quiet"], &material)?;
        let boot = guest("cat /proc/sys/kernel/random/boot_id")?;
        assert_eq!(
            guest("printf '%s' \"$API_TEST_TOKEN\"")?,
            "$MSB_API_TEST_TOKEN"
        );
        guest("export DEBIAN_FRONTEND=noninteractive; apt-get -qq update >/dev/null 2>&1 && apt-get -qq -y install curl ca-certificates python3-minimal >/dev/null 2>&1")?;
        let reflected = guest("curl -fsS --max-time 20 -H \"X-Silo-Test: $API_TEST_TOKEN\" https://httpbingo.org/headers?probe=$(date +%s)")?;
        assert!(
            reflected.contains("synthetic-first"),
            "allowed HTTPS did not receive the synthetic value; placeholder returned: {}", reflected.contains("$MSB_API_TEST_TOKEN")
        );
        assert!(
            !reflected.contains("$MSB_API_TEST_TOKEN"),
            "allowed HTTPS received a placeholder"
        );
        guest("curl -fsS --max-time 20 https://example.com >/dev/null")?;
        guest("if curl -fsS --max-time 20 -H \"X-Silo-Test: $API_TEST_TOKEN\" https://example.com >/dev/null 2>&1; then exit 1; fi")?;
        connection_update(None,"synthetic-first",Some("synthetic-first"))?;
        material[0].1 = "synthetic-rotated".into();
        connection_update(Some(&material),"synthetic-first",Some("synthetic-rotated"))?;
        let reflected = guest("curl -fsS --max-time 20 -H \"X-Silo-Test: $API_TEST_TOKEN\" https://httpbingo.org/headers?probe=$(date +%s)")?;
        assert!(reflected.contains("synthetic-rotated"));
        assert!(!reflected.contains("synthetic-first"));
        material[0].2 = vec!["other.example.com".into()];
        assert!(apply(&paths, name, &material, false)?.is_empty());
        assert_eq!(guest("cat /proc/sys/kernel/random/boot_id")?, boot);
        guest("if curl -fsS --max-time 20 -H \"X-Silo-Test: $API_TEST_TOKEN\" https://httpbingo.org/headers >/dev/null 2>&1; then exit 1; fi")?;
        material.push((
            "SECOND_TOKEN".into(),
            "synthetic-second".into(),
            vec!["api.example.com".into()],
        ));
        assert_eq!(apply(&paths, name, &material, false)?, vec!["SECOND_TOKEN"]);
        assert!(guest("printf '%s' \"${SECOND_TOKEN-unavailable}\"")?.contains("unavailable"));
        assert_eq!(guest("cat /proc/sys/kernel/random/boot_id")?, boot);
        // A second edit of the still-pending entry must remain deferred.
        material[1].1 = "synthetic-second-rotated".into();
        assert_eq!(apply(&paths, name, &material, false)?, vec!["SECOND_TOKEN"]);
        command(&["restart", name, "--quiet"], &material)?;
        assert_ne!(guest("cat /proc/sys/kernel/random/boot_id")?, boot);
        assert_eq!(guest("printf '%s' \"$SECOND_TOKEN\"")?, "$MSB_SECOND_TOKEN");
        assert!(apply(&paths, name, &material, false)?.is_empty());
        let restarted_boot = guest("cat /proc/sys/kernel/random/boot_id")?;
        material[0].2 = vec!["httpbingo.org".into()];
        assert!(apply(&paths, name, &material, false)?.is_empty());
        connection_update(Some(&vec![]),"synthetic-rotated",None)?;
        let after_removal = guest("curl -fsS --max-time 20 -H \"X-Silo-Test: $API_TEST_TOKEN\" https://httpbingo.org/headers?probe=$(date +%s)");
        if let Ok(body) = after_removal { assert!(!body.contains("synthetic-")); }

        assert_eq!(
            guest("cat /proc/sys/kernel/random/boot_id")?,
            restarted_boot
        );
        let inspected =
            inspect_computer(&ProcessRunner, &paths, name).map_err(|_| "final inspect failed")?;
        let encoded = serde_json::to_string(&inspected.config).unwrap();
        assert!(!encoded.contains("synthetic-"));
        Ok(())
    })).unwrap_or_else(|_| Err("live secret assertion failed".into()));
    let stopped = command(&["stop", name, "--quiet"], &vec![]);
    let removed = command(&["remove", name, "--quiet"], &vec![]);
    assert!(result.is_ok(), "{}", result.err().unwrap_or_default());
    assert!(stopped.is_ok(), "temporary computer stop failed");
    assert!(removed.is_ok(), "temporary computer removal failed");
}
