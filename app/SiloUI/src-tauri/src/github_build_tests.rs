#[path = "../github_build.rs"]
mod github_build;

fn fixture() -> tempfile::NamedTempFile {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), r#"{"SILO_GITHUB_APP_SLUG":"test-app","SILO_GITHUB_CLIENT_ID":"test-client","SILO_GITHUB_CLIENT_SECRET":"test-secret"}"#).unwrap();
    file
}

#[test]
fn local_configuration_works_without_shell_exports() {
    let file = fixture();
    let values = github_build::configuration(file.path(), |_| None).unwrap();
    assert_eq!(values["SILO_GITHUB_CLIENT_SECRET"], "test-secret");
}

#[test]
fn ci_environment_works_without_local_file() {
    let directory = tempfile::tempdir().unwrap();
    let values = github_build::configuration(&directory.path().join("absent.json"), |key| {
        Some(format!("ci-{key}"))
    })
    .unwrap();
    assert_eq!(values.len(), 3);
}

#[test]
fn environment_overrides_local_values_and_empty_secrets_fail() {
    let file = fixture();
    let values = github_build::configuration(file.path(), |_| Some("override".into())).unwrap();
    assert_eq!(values["SILO_GITHUB_CLIENT_SECRET"], "override");
    let error = github_build::configuration(file.path(), |key| {
        (key == "SILO_GITHUB_CLIENT_SECRET").then(String::new)
    })
    .unwrap_err();
    assert!(error.contains("SILO_GITHUB_CLIENT_SECRET"));
    assert!(!error.contains("test-secret"));
}

#[test]
fn missing_and_malformed_files_fail_without_echoing_values() {
    let directory = tempfile::tempdir().unwrap();
    assert!(github_build::configuration(&directory.path().join("absent.json"), |_| None).is_err());
    let file = fixture();
    std::fs::write(file.path(), "private-value-invalid-json").unwrap();
    let error = github_build::configuration(file.path(), |_| None).unwrap_err();
    assert!(!error.contains("private-value"));
}

#[test]
fn malformed_value_fails_with_key_name_only_and_does_not_echo_value() {
    let file = fixture();
    // Non-string JSON value in local file
    std::fs::write(
        file.path(),
        r#"{"SILO_GITHUB_APP_SLUG":"test-app","SILO_GITHUB_CLIENT_ID":"test-client","SILO_GITHUB_CLIENT_SECRET":123456789}"#,
    )
    .unwrap();
    let error = github_build::configuration(file.path(), |_| None).unwrap_err();
    assert!(error.contains("SILO_GITHUB_CLIENT_SECRET"));
    assert!(!error.contains("123456789"));

    // String with newline / carriage return / null
    std::fs::write(
        file.path(),
        r#"{"SILO_GITHUB_APP_SLUG":"test-app","SILO_GITHUB_CLIENT_ID":"test-client","SILO_GITHUB_CLIENT_SECRET":"secret\nvalue"}"#,
    )
    .unwrap();
    let error = github_build::configuration(file.path(), |_| None).unwrap_err();
    assert!(error.contains("SILO_GITHUB_CLIENT_SECRET"));
    assert!(!error.contains("secret"));

    // Malformed environment value
    let file = fixture();
    let error = github_build::configuration(file.path(), |key| {
        (key == "SILO_GITHUB_CLIENT_SECRET").then(|| "malformed-secret\n".to_string())
    })
    .unwrap_err();
    assert!(error.contains("SILO_GITHUB_CLIENT_SECRET"));
    assert!(!error.contains("malformed-secret"));
}

#[test]
fn cargo_directive_injection_is_rejected() {
    let file = fixture();
    assert!(
        github_build::configuration(file.path(), |_| Some("value\ncargo:injection".into()))
            .is_err()
    );
}
