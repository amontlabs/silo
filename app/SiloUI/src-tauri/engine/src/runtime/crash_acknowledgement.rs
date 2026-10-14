//! Acknowledge one observed crash without editing MicroSandbox's state or disks.
use super::*;
use serde_json::json;

fn computer_configuration(
    paths: &RuntimePaths,
    name: &str,
) -> Result<ComputerConfiguration, RuntimeError> {
    validate_name(name)?;
    read_metadata(&paths.metadata)?
        .computers
        .into_iter()
        .find(|m| m.name() == name)
        .ok_or_else(|| RuntimeError::Invalid("This computer no longer exists.".into()))
}
fn path(paths: &RuntimePaths, configuration: &ComputerConfiguration) -> PathBuf {
    paths
        .metadata
        .with_file_name("acknowledged-crashes")
        .join(format!(
            "{:x}.json",
            Sha256::digest(configuration.id().as_bytes())
        ))
}
fn fingerprint(
    configuration: &ComputerConfiguration,
    inspected: &InspectedSandbox,
) -> Option<String> {
    if inspected.name != configuration.name()
        || inspected
            .config
            .pointer("/labels/silo.machine-id")
            .and_then(Value::as_str)
            != Some(configuration.id())
    {
        return None;
    }
    let updated = inspected.updated_at.as_ref().filter(|s| !s.is_empty())?;
    Some(json!([configuration.id(), configuration.name(), updated]).to_string())
}
pub(super) fn is_acknowledged(
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
    inspected: &InspectedSandbox,
) -> bool {
    fingerprint(configuration, inspected).is_some_and(|expected| {
        fs::read_to_string(path(paths, configuration)).is_ok_and(|saved| saved == expected)
    })
}
pub(super) fn dismiss(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    name: &str,
) -> Result<(), RuntimeError> {
    let configuration = computer_configuration(paths, name)?;
    let inspected = inspect_computer(runner, paths, name)?;
    ensure_managed(&inspected)?;
    if inspected.name != name
        || inspected
            .config
            .pointer("/labels/silo.machine-id")
            .and_then(Value::as_str)
            != Some(configuration.id())
    {
        return Err(RuntimeError::Invalid(
            "The computer was replaced. Refresh before dismissing its error.".into(),
        ));
    }
    if inspected.status != "Crashed" {
        return Err(RuntimeError::Invalid(
            "The computer state changed. Refresh to see its current status.".into(),
        ));
    }
    let value = fingerprint(&configuration, &inspected).ok_or_else(|| {
        RuntimeError::Invalid("The runtime did not identify this crash. Refresh and retry.".into())
    })?;
    lifecycle_recovery::dismiss_crashed_intent(paths, &configuration)?;
    let target = path(paths, &configuration);
    let directory = target.parent().unwrap();
    let save = || -> std::io::Result<()> {
        fs::create_dir_all(directory)?;
        let mut file = tempfile::NamedTempFile::new_in(directory)?;
        use std::io::Write;
        file.write_all(value.as_bytes())?;
        file.as_file().sync_all()?;
        file.persist(&target)?;
        File::open(directory)?.sync_all()
    };
    save().map_err(|_| {
        RuntimeError::Unavailable("The crash could not be dismissed. Retry.".into())
    })?;
    runtime_activity::acknowledge_failure(paths, configuration.id())
}
pub(super) fn clear(paths: &RuntimePaths, name: &str) -> Result<(), RuntimeError> {
    let configuration = computer_configuration(paths, name)?;
    match fs::remove_file(path(paths, &configuration)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(RuntimeError::Unavailable(
            "The previous crash acknowledgement could not be cleared.".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "00000000-0000-4000-8000-0000000000c1";
    const AT: Option<&str> = Some("2026-09-16T00:00:00.123Z");

    fn configured(dir: &tempfile::TempDir, id: &str) -> (RuntimePaths, ComputerConfiguration) {
        let paths = super::super::tests::paths(dir);
        let configuration: ComputerConfiguration = serde_json::from_value(json!({"id":id,"name":"dev","cpus":1,"maxCPUs":1,"memoryGiB":1,"maxMemoryGiB":1,"workspaceStorageGiB":1,"runtimeStorageGiB":1})).unwrap();
        write_metadata(
            &paths.metadata,
            &ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![configuration.clone()],
            },
        )
        .unwrap();
        (paths, configuration)
    }
    fn report(status: &str, id: &str, updated_at: Option<&str>) -> Value {
        json!({"name":"dev","status":status,"updated_at":updated_at,
            "config":{"labels":{"silo.managed":"true","silo.machine-id":id}}})
    }
    fn crash(id: &str, updated_at: Option<&str>) -> InspectedSandbox {
        serde_json::from_value(report("Crashed", id, updated_at)).unwrap()
    }
    /// Answers only `inspect`: dismissal must never change the computer.
    struct Inspect(Value);
    impl RuntimeRunner for Inspect {
        fn run(
            &self,
            _: &RuntimePaths,
            args: &[String],
            _: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            assert_eq!(args[0], "inspect");
            Ok(CommandOutput {
                stdout: self.0.to_string(),
                stderr: String::new(),
            })
        }
    }

    #[test]
    fn an_acknowledgement_hides_only_the_crash_it_was_given_for() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, configuration) = configured(&dir, ID);
        dismiss(&Inspect(report("Crashed", ID, AT)), &paths, "dev").unwrap();
        assert!(is_acknowledged(&paths, &configuration, &crash(ID, AT)));
        // A later crash of the same computer is a new crash.
        assert!(!is_acknowledged(
            &paths,
            &configuration,
            &crash(ID, Some("2026-09-16T00:00:00.124Z"))
        ));
        // A crash reported without a timestamp can never match an acknowledgement.
        assert!(!is_acknowledged(&paths, &configuration, &crash(ID, None)));
        // Another runtime identity under the same name is not the acknowledged computer.
        assert!(!is_acknowledged(
            &paths,
            &configuration,
            &crash("replacement", AT)
        ));
    }

    #[test]
    fn corrupt_activity_history_does_not_fail_a_saved_crash_acknowledgement() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, configuration) = configured(&dir, ID);
        let history = paths.metadata.with_file_name("computer-activity.json");
        let original = b"{unfinished activity history";
        fs::write(&history, original).unwrap();

        dismiss(&Inspect(report("Crashed", ID, AT)), &paths, "dev").unwrap();
        assert!(is_acknowledged(&paths, &configuration, &crash(ID, AT)));
        assert_eq!(fs::read(&history).unwrap(), original);
        assert!(runtime_activity::read(&paths).unwrap().iter().any(|entry| {
            entry["id"] == "computer-history-unavailable" && entry["tone"] == "warning"
        }));
    }

    #[test]
    fn a_recreated_computer_with_the_same_name_does_not_inherit_an_acknowledgement() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, _) = configured(&dir, ID);
        dismiss(&Inspect(report("Crashed", ID, AT)), &paths, "dev").unwrap();
        let other = "00000000-0000-4000-8000-0000000000c2";
        let (paths, recreated) = configured(&dir, other);
        assert!(!is_acknowledged(&paths, &recreated, &crash(other, AT)));
    }

    #[test]
    fn fingerprints_do_not_collide_across_field_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        let (_, configuration) = configured(&dir, ID);
        let plain = fingerprint(&configuration, &crash(ID, Some("1,2"))).unwrap();
        let quoted = fingerprint(&configuration, &crash(ID, Some("1\",\"2"))).unwrap();
        assert_ne!(plain, quoted);
        assert_eq!(
            serde_json::from_str::<Vec<String>>(&quoted).unwrap(),
            vec![ID, "dev", "1\",\"2"]
        );
    }

    #[test]
    fn clearing_restores_the_crash_and_tolerates_a_missing_acknowledgement() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, configuration) = configured(&dir, ID);
        clear(&paths, "dev").unwrap();
        dismiss(&Inspect(report("Crashed", ID, AT)), &paths, "dev").unwrap();
        clear(&paths, "dev").unwrap();
        assert!(!is_acknowledged(&paths, &configuration, &crash(ID, AT)));
        assert!(clear(&paths, "missing").is_err());
    }

    #[test]
    fn dismissal_rejects_a_crash_it_cannot_identify_and_saves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, configuration) = configured(&dir, ID);
        for observed in [
            report("Crashed", ID, None),
            report("Crashed", "replacement", AT),
            report("Running", ID, AT),
        ] {
            assert!(dismiss(&Inspect(observed), &paths, "dev").is_err());
        }
        assert!(!path(&paths, &configuration).exists());
    }
}
