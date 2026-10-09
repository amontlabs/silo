use serde_json::Value;

#[test]
fn macos_computer_commands_belong_to_the_main_window_only() {
    let manifests: Value =
        serde_json::from_str(include_str!("../gen/schemas/acl-manifests.json")).unwrap();
    let capabilities: Value =
        serde_json::from_str(include_str!("../gen/schemas/capabilities.json")).unwrap();
    let granted_to = |permission: &str| -> Vec<String> {
        capabilities
            .as_object()
            .unwrap()
            .iter()
            .filter(|(_, capability)| {
                capability["permissions"]
                    .as_array()
                    .unwrap()
                    .contains(&Value::String(permission.into()))
            })
            .map(|(name, _)| name.clone())
            .collect()
    };
    for command in [
        "read_macos_computers",
        "create_macos_computer",
        "macos_computer_action",
        "open_macos_display",
        "macos_computer_clipboard",
        "delete_macos_template",
    ] {
        let permission = format!("allow-{}", command.replace('_', "-"));
        assert_eq!(
            manifests["__app-acl__"]["permissions"][&permission]["commands"]["allow"],
            serde_json::json!([command]),
            "Tauri must generate a permission for {command}"
        );
        assert_eq!(granted_to(&permission), ["preview"], "{command}");
    }
    assert_eq!(
        capabilities["preview"]["windows"],
        serde_json::json!(["main"])
    );
    let events = &capabilities["application-events"];
    assert!(events["windows"]
        .as_array()
        .unwrap()
        .contains(&Value::String("main".into())));
    assert!(events["permissions"]
        .as_array()
        .unwrap()
        .contains(&Value::String("core:event:allow-listen".into())));
    // A macOS display window is labelled `macos-display-<id>`; no capability may match it.
    let label = "macos-display-00000000-0000-0000-0000-000000000000";
    for (name, capability) in capabilities.as_object().unwrap() {
        let matches = |key: &str| {
            capability[key].as_array().is_some_and(|patterns| {
                patterns.iter().filter_map(Value::as_str).any(|pattern| {
                    pattern == "*"
                        || pattern == label
                        || pattern
                            .strip_suffix('*')
                            .is_some_and(|prefix| label.starts_with(prefix))
                })
            })
        };
        assert!(
            !matches("windows") && !matches("webviews"),
            "{name} would grant IPC to macOS display windows"
        );
    }
}

#[test]
fn material_preview_theme_command_is_not_granted_to_production_windows() {
    let manifests: Value =
        serde_json::from_str(include_str!("../gen/schemas/acl-manifests.json")).unwrap();
    let capabilities: Value =
        serde_json::from_str(include_str!("../gen/schemas/capabilities.json")).unwrap();
    assert_eq!(
        manifests["__app-acl__"]["permissions"]["allow-set-preview-theme"]["commands"]["allow"],
        serde_json::json!(["set_preview_theme"]),
    );
    for capability in capabilities.as_object().unwrap().values() {
        assert!(
            !capability["permissions"]
                .as_array()
                .unwrap()
                .contains(&Value::String("allow-set-preview-theme".into())),
            "Only the example harness can grant its theme command at runtime",
        );
    }
}

#[test]
fn migration_and_checkpoint_commands_are_allowlisted_for_the_main_window() {
    let manifests: Value =
        serde_json::from_str(include_str!("../gen/schemas/acl-manifests.json")).unwrap();
    let capabilities: Value =
        serde_json::from_str(include_str!("../gen/schemas/capabilities.json")).unwrap();
    let preview = &capabilities["preview"];
    assert_eq!(preview["windows"], serde_json::json!(["main"]));
    assert_eq!(preview["local"], serde_json::json!(true));

    for command in [
        "read_runtime_migration_state",
        "retry_runtime_migration",
        "continue_after_migration_failure",
        "create_checkpoint",
        "fork_checkpoint",
        "restore_checkpoint",
        "delete_checkpoint",
        "abandon_restore",
        "read_checkpoint_usage",
        "remote_checkpoint_action",
        "acknowledge_backup_result",
    ] {
        let permission = format!("allow-{}", command.replace('_', "-"));
        assert_eq!(
            manifests["__app-acl__"]["permissions"][&permission]["commands"]["allow"],
            serde_json::json!([command]),
            "Tauri must generate a permission for {command}"
        );
        assert!(
            preview["permissions"]
                .as_array()
                .unwrap()
                .contains(&Value::String(permission.clone())),
            "The local main window must be able to invoke {command}"
        );
    }
}

#[test]
fn reveal_backup_archive_is_allowlisted_for_the_main_window_only() {
    let manifests: Value =
        serde_json::from_str(include_str!("../gen/schemas/acl-manifests.json")).unwrap();
    let capabilities: Value =
        serde_json::from_str(include_str!("../gen/schemas/capabilities.json")).unwrap();
    let preview = &capabilities["preview"];
    assert_eq!(preview["windows"], serde_json::json!(["main"]));
    assert_eq!(preview["local"], serde_json::json!(true));

    assert_eq!(
        manifests["__app-acl__"]["permissions"]["allow-reveal-backup-archive"]["commands"]["allow"],
        serde_json::json!(["reveal_backup_archive"]),
        "Tauri must generate a permission for reveal_backup_archive"
    );
    assert!(
        preview["permissions"]
            .as_array()
            .unwrap()
            .contains(&Value::String("allow-reveal-backup-archive".into())),
        "The local main window must be able to invoke reveal_backup_archive"
    );

    // No other capability may grant the command to a non-main window.
    for (name, capability) in capabilities.as_object().unwrap() {
        if name == "preview" {
            continue;
        }
        assert!(
            !capability["permissions"]
                .as_array()
                .unwrap()
                .contains(&Value::String("allow-reveal-backup-archive".into())),
            "Only the main window may reveal export files, not {name}"
        );
    }
}

#[test]
fn pre_upgrade_backup_commands_are_allowlisted_for_the_main_window_only() {
    let manifests: Value =
        serde_json::from_str(include_str!("../gen/schemas/acl-manifests.json")).unwrap();
    let capabilities: Value =
        serde_json::from_str(include_str!("../gen/schemas/capabilities.json")).unwrap();
    let preview = &capabilities["preview"];
    assert_eq!(preview["windows"], serde_json::json!(["main"]));

    for command in [
        "read_pre_upgrade_backup",
        "measure_pre_upgrade_backup",
        "delete_pre_upgrade_backup",
        "reveal_pre_upgrade_backup",
        "acknowledge_pre_upgrade_backup_notice",
    ] {
        let permission = format!("allow-{}", command.replace('_', "-"));
        assert_eq!(
            manifests["__app-acl__"]["permissions"][&permission]["commands"]["allow"],
            serde_json::json!([command]),
            "Tauri must generate a permission for {command}"
        );
        assert!(
            preview["permissions"]
                .as_array()
                .unwrap()
                .contains(&Value::String(permission.clone())),
            "The local main window must be able to invoke {command}"
        );
        // No other capability may grant a deletion or file-manager command to another window.
        for (name, capability) in capabilities.as_object().unwrap() {
            if name == "preview" {
                continue;
            }
            assert!(
                !capability["permissions"]
                    .as_array()
                    .unwrap()
                    .contains(&Value::String(permission.clone())),
                "Only the main window may use {command}, not {name}"
            );
        }
    }
}

#[test]
fn file_transfer_commands_are_limited_to_the_main_window_and_the_viewer_shell() {
    let manifests: Value =
        serde_json::from_str(include_str!("../gen/schemas/acl-manifests.json")).unwrap();
    let capabilities: Value =
        serde_json::from_str(include_str!("../gen/schemas/capabilities.json")).unwrap();
    let holders = |permission: &str| -> Vec<String> {
        let mut names: Vec<String> = capabilities
            .as_object()
            .unwrap()
            .iter()
            .filter(|(_, capability)| {
                capability["permissions"]
                    .as_array()
                    .unwrap()
                    .contains(&Value::String(permission.into()))
            })
            .map(|(name, _)| name.clone())
            .collect();
        names.sort();
        names
    };
    for (command, viewer) in [
        ("choose_upload_files", false),
        ("download_file", false),
        ("upload_files", true),
        ("cancel_transfer", true),
    ] {
        let permission = format!("allow-{}", command.replace('_', "-"));
        assert_eq!(
            manifests["__app-acl__"]["permissions"][&permission]["commands"]["allow"],
            serde_json::json!([command]),
            "Tauri must generate a permission for {command}"
        );
        let expected: &[&str] = if viewer {
            &["desktop-transfer", "preview"]
        } else {
            &["preview"]
        };
        assert_eq!(holders(&permission), expected, "{command}");
    }
    assert_eq!(
        capabilities["desktop-transfer"]["webviews"],
        serde_json::json!(["desktop-shell-*"])
    );
    assert!(capabilities["desktop-transfer"]["windows"]
        .as_array()
        .is_none_or(Vec::is_empty));
}
