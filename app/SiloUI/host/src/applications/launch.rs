//! Platform-neutral launch policy, unit-tested on every host: how a Linux
//! desktop entry maps to the command Silo can run, and the environment child
//! processes receive when Silo runs as an AppImage. `linux.rs` supplies GIO.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]
use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::Command,
};

/// An editor command that accepts a computer folder.
#[derive(Debug, PartialEq)]
pub struct EditorCommand {
    pub program: PathBuf,
    /// Arguments before Silo's own (for example `run <app id>` for Flatpak).
    pub args: Vec<OsString>,
    /// Zed takes an `ssh://` URI; Visual Studio Code takes a workspace file.
    pub zed: bool,
}

const UNSUPPORTED_EDITOR: &str =
    "Remote folders currently support Zed and Visual Studio Code. Choose one in Settings.";

/// A desktop entry's `Exec` tokens without field codes (`%U`) or Flatpak's
/// file-forwarding markers (`@@`, `@@u`).
pub fn exec_argv(tokens: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut argv: Vec<_> = tokens
        .into_iter()
        .filter(|token| {
            !(token.len() == 2 && token.starts_with('%') && token != "%%")
                && !matches!(token.as_str(), "@@" | "@@u")
        })
        // Expand literal percent escapes once, after removing field codes.
        // An escaped %%F is a literal %F argument, not a file placeholder.
        .map(|token| token.replace("%%", "%"))
        .collect();
    // Field-code removal can leave an empty file-argument section. Silo's
    // appended options must still be parsed as options by the editor.
    if argv.last().is_some_and(|argument| argument == "--") {
        argv.pop();
    }
    argv
}

/// The program an `Exec` line runs after env options and assignments.
pub fn exec_program(argv: &[String]) -> Option<&str> {
    exec_program_index(argv).map(|index| argv[index].as_str())
}

fn exec_program_index(argv: &[String]) -> Option<usize> {
    let mut tokens = argv.iter().enumerate();
    let (_, first) = tokens.next()?;
    if Path::new(first).file_name() != Some(OsStr::new("env")) {
        return Some(0);
    }
    while let Some((index, token)) = tokens.next() {
        if matches!(token.as_str(), "-u" | "--unset" | "-C" | "--chdir") {
            tokens.next()?;
        } else if !token.starts_with('-') && !token.contains('=') {
            return Some(index);
        }
    }
    None
}

fn file_name(path: &Path) -> &str {
    path.file_name().and_then(OsStr::to_str).unwrap_or("")
}

pub fn executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

fn with_exec_prefix(
    argv: &[String],
    index: usize,
    command: EditorCommand,
    find_program: &dyn Fn(&str) -> Option<PathBuf>,
) -> Result<EditorCommand, String> {
    if index == 0 {
        return Ok(command);
    }
    let program = if Path::new(&argv[0]).is_absolute() {
        PathBuf::from(&argv[0])
    } else {
        find_program(&argv[0]).ok_or("The selected editor is unavailable.")?
    };
    if !executable_file(&program) {
        return Err("The selected editor is unavailable.".into());
    }
    let mut args: Vec<_> = argv[1..index].iter().map(OsString::from).collect();
    args.push(command.program.into_os_string());
    args.extend(command.args);
    Ok(EditorCommand {
        program,
        args,
        zed: command.zed,
    })
}

/// Resolves a Linux editor entry to its command-line launcher (G-06, G-25).
///
/// The Microsoft package's entry runs the Electron app (`/usr/share/code/code`),
/// which never exits; its CLI is `bin/code` beside it. Zed's tarball entry runs
/// `libexec/zed-editor`, whose CLI is `bin/zed`. Flatpak entries run
/// `flatpak run <app id>`, and snap entries prefix `env NAME=value`.
pub fn linux_editor_command(
    argv: &[String],
    flatpak_app: Option<&str>,
    find_program: &dyn Fn(&str) -> Option<PathBuf>,
) -> Result<EditorCommand, String> {
    if let Some(app) = flatpak_app {
        let zed = match app {
            "com.visualstudio.code" => false,
            "dev.zed.Zed" => true,
            _ => return Err(UNSUPPORTED_EDITOR.into()),
        };
        let index = exec_program_index(argv).ok_or("The selected editor is unavailable.")?;
        let token = &argv[index];
        if file_name(Path::new(token)) != "flatpak" {
            return Err("The selected editor is unavailable.".into());
        }
        let args = &argv[index + 1..];
        if args.first().map(String::as_str) != Some("run")
            || !args[1..].iter().any(|argument| argument == app)
        {
            return Err("The selected editor is unavailable.".into());
        }
        let program = if Path::new(token).is_absolute() {
            PathBuf::from(token)
        } else {
            find_program(token).ok_or("The selected editor is unavailable.")?
        };
        if !executable_file(&program) {
            return Err("The selected editor is unavailable.".into());
        }
        return with_exec_prefix(
            argv,
            index,
            EditorCommand {
                program,
                args: args
                    .iter()
                    .filter(|argument| argument.as_str() != "--file-forwarding")
                    .map(OsString::from)
                    .collect(),
                zed,
            },
            find_program,
        );
    }
    let index = exec_program_index(argv).ok_or("The selected editor is unavailable.")?;
    let token = &argv[index];
    let program = if Path::new(token).is_absolute() {
        PathBuf::from(token)
    } else {
        find_program(token).ok_or("The selected editor is unavailable.")?
    };
    let directory = program.parent().unwrap_or(Path::new("/"));
    let (program, zed) = match file_name(&program) {
        name @ ("code" | "code-insiders") => {
            let cli = directory.join("bin").join(name);
            (if executable_file(&cli) { cli } else { program }, false)
        }
        "zed" | "zeditor" => (program, true),
        "zed-editor" => {
            let cli = directory.join("../bin/zed");
            (if executable_file(&cli) { cli } else { program }, true)
        }
        _ => return Err(UNSUPPORTED_EDITOR.into()),
    };
    if !executable_file(&program) {
        return Err("The selected editor is unavailable.".into());
    }
    with_exec_prefix(
        argv,
        index,
        EditorCommand {
            program,
            args: argv[index + 1..].iter().map(OsString::from).collect(),
            zed,
        },
        find_program,
    )
}

/// Resolves the actual terminal executable behind an entry's env wrapper.
pub fn linux_terminal_program(
    argv: &[String],
    fallback: PathBuf,
    find_program: &dyn Fn(&str) -> Option<PathBuf>,
) -> Option<PathBuf> {
    let program = match exec_program(argv) {
        Some(token) if Path::new(token).is_absolute() => Some(PathBuf::from(token)),
        Some(token) => find_program(token),
        None => Some(fallback),
    }?;
    executable_file(&program).then_some(program)
}

/// Commands that run the user's preferred terminal (G-07). The freedesktop
/// `xdg-terminal-exec` comes first, then Debian's `x-terminal-emulator`.
pub const TERMINAL_LAUNCHERS: [&str; 2] = ["xdg-terminal-exec", "x-terminal-emulator"];

/// The terminal default: a system launcher when one exists, otherwise the
/// first listed terminal Silo can drive.
pub fn linux_terminal_default(
    find_program: &dyn Fn(&str) -> Option<PathBuf>,
    supported_in_order: &[String],
) -> Option<String> {
    TERMINAL_LAUNCHERS
        .iter()
        .find_map(|launcher| find_program(launcher))
        .and_then(|path| path.to_str().map(str::to_owned))
        .or_else(|| supported_in_order.first().cloned())
}

/// The AppImage's mount point, when this process runs from one.
fn appimage_root() -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var_os("APPDIR")?);
    std::env::var_os("APPIMAGE")?;
    let executable = std::env::current_exe().ok()?;
    (root.is_absolute() && executable.starts_with(&root)).then_some(root)
}

/// Environment changes that undo the AppImage's AppRun hooks for a child
/// (G-24): every variable naming the mount point loses those entries (and is
/// removed when nothing remains), so children such as gnome-terminal use the
/// system's GSettings schemas, GTK modules, GStreamer plugins and libraries.
pub fn appimage_child_environment(
    variables: impl IntoIterator<Item = (OsString, OsString)>,
    root: &Path,
) -> Vec<(OsString, Option<OsString>)> {
    if root.as_os_str().is_empty() || root == Path::new("/") {
        return Vec::new();
    }
    let mut changes = Vec::new();
    for (name, value) in variables {
        if matches!(name.to_str(), Some("APPDIR" | "APPIMAGE" | "ARGV0" | "OWD")) {
            changes.push((name, None));
            continue;
        }
        let entries: Vec<_> = std::env::split_paths(&value).collect();
        let kept: Vec<_> = entries
            .iter()
            .filter(|entry| !entry.starts_with(root))
            .collect();
        if kept.len() == entries.len() {
            continue;
        }
        let kept: Vec<_> = kept
            .into_iter()
            .filter(|entry| !entry.as_os_str().is_empty())
            .collect();
        changes.push((
            name,
            (!kept.is_empty()).then(|| {
                std::env::join_paths(kept).expect("Paths from split_paths contain no separator")
            }),
        ));
    }
    changes
}

/// The changes for this process's children; empty outside an AppImage.
pub fn child_environment() -> Vec<(OsString, Option<OsString>)> {
    appimage_root()
        .map(|root| appimage_child_environment(std::env::vars_os(), &root))
        .unwrap_or_default()
}

/// Gives a child the system environment instead of the AppImage's (G-24).
pub fn sanitize_child(command: &mut Command) -> &mut Command {
    for (name, value) in child_environment() {
        match value {
            Some(value) => command.env(name, value),
            None => command.env_remove(name),
        };
    }
    command
}

/// A path to Silo that outlives this process: the AppImage file rather than
/// its temporary mount (G-12). Elsewhere, the running executable.
pub fn stable_executable() -> Result<PathBuf, String> {
    if appimage_root().is_some() {
        if let Some(image) = std::env::var_os("APPIMAGE").map(PathBuf::from) {
            if image.is_absolute() && executable_file(&image) {
                return Ok(image);
            }
        }
    }
    std::env::current_exe().map_err(|_| "Silo could not locate its executable.".into())
}

/// Whether persisted commands must go through `stable_executable` because
/// bundled tool paths are temporary (AppImage mounts).
pub fn tools_are_temporary() -> bool {
    appimage_root().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn tokens(line: &str) -> Vec<String> {
        exec_argv(line.split_whitespace().map(str::to_owned))
    }
    fn executable(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    fn nowhere(_: &str) -> Option<PathBuf> {
        None
    }

    #[test]
    fn exec_lines_drop_field_codes_and_skip_env_prefixes() {
        assert_eq!(tokens("/usr/share/code/code %F"), ["/usr/share/code/code"]);
        let snap = tokens("env BAMF_DESKTOP_FILE_HINT=/var/lib/snapd/code.desktop /snap/bin/code --force-user-env %F");
        assert_eq!(exec_program(&snap), Some("/snap/bin/code"));
        assert_eq!(
            exec_program(&tokens("/usr/bin/env -i A=b zed %U")),
            Some("zed")
        );
        let flatpak = tokens(
            "/usr/bin/flatpak run --branch=stable --command=code com.visualstudio.code @@ %F @@",
        );
        assert!(!flatpak.iter().any(|token| token == "@@" || token == "%F"));
    }

    #[test]
    fn env_working_directory_operands_do_not_replace_the_editor_program() {
        let directory = tempfile::tempdir().unwrap();
        let cli = directory.path().join("code");
        executable(&cli);
        std::fs::write(&cli, "#!/bin/sh\npwd\nprintf '%s\\n' \"$1\"\n").unwrap();
        for option in ["-C", "--chdir"] {
            let argv = vec![
                "/usr/bin/env".into(),
                option.into(),
                directory.path().to_str().unwrap().into(),
                cli.to_str().unwrap().into(),
            ];
            assert_eq!(exec_program(&argv), cli.to_str(), "{option}");
            let launch = linux_editor_command(&argv, None, &nowhere).unwrap();
            // macOS env supports the short option; GNU env on Linux supports both.
            if option == "-C" || cfg!(target_os = "linux") {
                let output = Command::new(launch.program)
                    .args(launch.args)
                    .arg("--profile")
                    .output()
                    .unwrap();
                assert!(output.status.success());
                assert_eq!(
                    String::from_utf8(output.stdout).unwrap(),
                    format!(
                        "{}\n--profile\n",
                        directory.path().canonicalize().unwrap().display()
                    )
                );
            }
        }
    }

    #[test]
    fn env_wrapped_terminals_require_an_available_target() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let program = directory.path().join("gnome-terminal");
        let argv = vec![
            "/usr/bin/env".into(),
            "TERM=xterm".into(),
            program.to_str().unwrap().into(),
        ];
        let resolve = || linux_terminal_program(&argv, PathBuf::from("/usr/bin/env"), &nowhere);
        assert!(resolve().is_none(), "removed terminal was accepted");
        executable(&program);
        assert_eq!(resolve(), Some(program.clone()));
        fs::set_permissions(&program, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(resolve().is_none(), "non-executable terminal was accepted");
        fs::remove_file(&program).unwrap();
        assert!(resolve().is_none());
    }

    #[test]
    fn desktop_percent_escapes_preserve_the_editor_path_and_arguments() {
        let directory = tempfile::tempdir().unwrap();
        let program = directory.path().join("100%/code");
        executable(&program);
        std::fs::write(&program, "#!/bin/sh\nprintf '%s\\n' \"$@\"\n").unwrap();
        let escaped_program = program.to_str().unwrap().replace('%', "%%");
        let argv = exec_argv([
            escaped_program,
            "--user-data-dir=/tmp/100%%".into(),
            "%%".into(),
            "%%F".into(),
            "%F".into(),
        ]);
        let command = linux_editor_command(&argv, None, &nowhere).unwrap();
        let output = Command::new(command.program)
            .args(command.args)
            .arg("fixture.code-workspace")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "--user-data-dir=/tmp/100%\n%\n%F\nfixture.code-workspace\n"
        );
    }

    #[test]
    fn the_microsoft_package_entry_resolves_to_the_code_cli() {
        let root = tempfile::tempdir().unwrap();
        let electron = root.path().join("usr/share/code/code");
        executable(&electron);
        let cli = root.path().join("usr/share/code/bin/code");
        executable(&cli);
        let argv = vec![electron.to_str().unwrap().to_owned()];
        let command = linux_editor_command(&argv, None, &nowhere).unwrap();
        assert_eq!(
            command,
            EditorCommand {
                program: cli,
                args: Vec::new(),
                zed: false
            }
        );
        // A CLI on PATH (the package's /usr/bin/code link) is used as is.
        let path_cli = root.path().join("bin/code");
        executable(&path_cli);
        let found = path_cli.clone();
        let command =
            linux_editor_command(&["code".into()], None, &move |_: &str| Some(found.clone()))
                .unwrap();
        assert_eq!(command.program, path_cli);
    }

    #[test]
    fn env_unset_operands_are_not_confused_with_the_editor_program() {
        let directory = tempfile::tempdir().unwrap();
        let program = directory.path().join("code");
        executable(&program);
        std::fs::write(
            &program,
            "#!/bin/sh\nprintf '%s\\n' \"${code-unset}\" \"$@\"\n",
        )
        .unwrap();
        let argv = ["/usr/bin/env", "-u", "code", "code"].map(str::to_owned);
        let command = linux_editor_command(&argv, None, &|_| Some(program.clone())).unwrap();
        let output = Command::new(command.program)
            .env("PATH", directory.path())
            .env("code", "must be removed")
            .args(command.args)
            .arg("fixture.code-workspace")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "unset\nfixture.code-workspace\n"
        );
        for flag in ["-u", "--unset"] {
            assert_eq!(
                exec_program(&["env", flag, "EXAMPLE", "code"].map(str::to_owned)),
                Some("code")
            );
            assert!(exec_program(&["env", flag].map(str::to_owned)).is_none());
        }
    }

    #[test]
    fn editor_entries_preserve_their_environment_wrapper_at_launch() {
        let directory = tempfile::tempdir().unwrap();
        for (name, app) in [("code", None), ("flatpak", Some("com.visualstudio.code"))] {
            let program = directory.path().join(name);
            executable(&program);
            std::fs::write(
                &program,
                "#!/bin/sh\nprintf '%s\\n' \"$SILO_EDITOR_ENTRY_TEST\" \"$@\"\n",
            )
            .unwrap();
            let mut argv = vec![
                "/usr/bin/env".into(),
                "SILO_EDITOR_ENTRY_TEST=selected environment".into(),
                program.to_str().unwrap().into(),
            ];
            if app.is_some() {
                argv.extend(["run".into(), "com.visualstudio.code".into()]);
            }
            let command = linux_editor_command(&argv, app, &nowhere).unwrap();
            let output = Command::new(command.program)
                .env_remove("SILO_EDITOR_ENTRY_TEST")
                .args(command.args)
                .arg("fixture.code-workspace")
                .output()
                .unwrap();
            assert!(output.status.success());
            let text = String::from_utf8(output.stdout).unwrap();
            assert!(
                text.starts_with("selected environment\n"),
                "{name}: {text:?}"
            );
            assert!(text.ends_with("fixture.code-workspace\n"));
        }
    }

    #[test]
    fn env_wrapped_editors_reject_missing_or_non_executable_launchers() {
        let directory = tempfile::tempdir().unwrap();
        let program = directory.path().join("code");
        let argv = vec![
            "/usr/bin/env".into(),
            "A=b".into(),
            program.to_str().unwrap().into(),
        ];
        assert!(linux_editor_command(&argv, None, &nowhere).is_err());
        std::fs::write(&program, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(linux_editor_command(&argv, None, &nowhere).is_err());
        executable(&program);
        assert!(linux_editor_command(&argv, None, &nowhere).is_ok());
    }

    #[test]
    fn flatpak_editor_preserves_the_selected_installation_and_command() {
        let directory = tempfile::tempdir().unwrap();
        let launcher = directory.path().join("flatpak");
        executable(&launcher);
        let find = |_: &str| Some(launcher.clone());
        for branch in ["stable", "beta"] {
            let argv = tokens(&format!(
                "{} run --user --branch={branch} --arch=aarch64 --command=code --file-forwarding com.visualstudio.code @@ %F @@", launcher.display()
            ));
            let command =
                linux_editor_command(&argv, Some("com.visualstudio.code"), &find).unwrap();
            assert_eq!(
                command.args,
                [
                    "run",
                    "--user",
                    &format!("--branch={branch}"),
                    "--arch=aarch64",
                    "--command=code",
                    "com.visualstudio.code"
                ]
                .map(OsString::from)
            );
        }
    }

    #[test]
    fn desktop_file_separators_do_not_hide_silos_editor_options() {
        let directory = tempfile::tempdir().unwrap();
        for name in ["code", "flatpak"] {
            executable(&directory.path().join(name));
        }
        let find = |name: &str| Some(directory.path().join(name));
        let native = linux_editor_command(&tokens("code -- %F"), None, &find).unwrap();
        assert!(native.args.is_empty());
        let flatpak = linux_editor_command(
            &tokens("flatpak run --command=code com.visualstudio.code -- @@ %F @@"),
            Some("com.visualstudio.code"),
            &find,
        )
        .unwrap();
        assert_eq!(
            flatpak.args,
            ["run", "--command=code", "com.visualstudio.code"].map(OsString::from)
        );
    }

    #[test]
    fn native_editor_entries_keep_their_isolated_data_and_extensions() {
        let directory = tempfile::tempdir().unwrap();
        let electron = directory.path().join("code/code");
        executable(&electron);
        let cli = directory.path().join("code/bin/code");
        executable(&cli);
        std::fs::write(&cli, "#!/bin/sh\nprintf '%s\\n' \"$@\"\n").unwrap();
        let argv = vec![
            electron.to_str().unwrap().to_owned(),
            "--user-data-dir=/tmp/isolated data".into(),
            "--extensions-dir=/tmp/isolated extensions".into(),
        ];
        let command = linux_editor_command(&argv, None, &nowhere).unwrap();
        let output = Command::new(command.program)
            .args(command.args)
            .args(["--profile", "Silo test", "fixture.code-workspace"])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "--user-data-dir=/tmp/isolated data\n--extensions-dir=/tmp/isolated extensions\n--profile\nSilo test\nfixture.code-workspace\n"
        );
    }

    #[test]
    fn snap_flatpak_and_zed_tarball_entries_are_launchable() {
        let directory = tempfile::tempdir().unwrap();
        let snap_program = directory.path().join("snap/bin/code");
        executable(&snap_program);
        let snap = tokens(&format!(
            "/usr/bin/env BAMF_DESKTOP_FILE_HINT=x {} --force-user-env %F",
            snap_program.display()
        ));
        let command = linux_editor_command(&snap, None, &nowhere).unwrap();
        assert_eq!(
            (command.program, command.args, command.zed),
            (
                PathBuf::from("/usr/bin/env"),
                vec![
                    "BAMF_DESKTOP_FILE_HINT=x".into(),
                    snap_program.into_os_string(),
                    "--force-user-env".into()
                ],
                false
            )
        );

        let launcher = directory.path().join("flatpak");
        executable(&launcher);
        let flatpak = |_: &str| Some(launcher.clone());
        let command = linux_editor_command(
            &tokens(&format!("{} run dev.zed.Zed %U", launcher.display())),
            Some("dev.zed.Zed"),
            &flatpak,
        )
        .unwrap();
        assert_eq!(
            command,
            EditorCommand {
                program: launcher.clone(),
                args: vec!["run".into(), "dev.zed.Zed".into()],
                zed: true
            }
        );
        let command = linux_editor_command(
            &tokens("flatpak run com.visualstudio.code %F"),
            Some("com.visualstudio.code"),
            &flatpak,
        )
        .unwrap();
        assert_eq!(
            command.args,
            [
                OsString::from("run"),
                OsString::from("com.visualstudio.code")
            ]
        );
        assert!(!command.zed);
        assert!(linux_editor_command(&[], Some("org.gnome.TextEditor"), &flatpak).is_err());

        let root = tempfile::tempdir().unwrap();
        let editor = root.path().join("zed.app/libexec/zed-editor");
        executable(&editor);
        executable(&root.path().join("zed.app/bin/zed"));
        let command =
            linux_editor_command(&[editor.to_str().unwrap().into()], None, &nowhere).unwrap();
        assert!(command.zed);
        assert!(command.program.ends_with("zed.app/libexec/../bin/zed"));
    }

    #[test]
    fn text_editors_that_cannot_open_computers_are_refused() {
        for line in [
            "gnome-text-editor %U",
            "/usr/bin/gedit %U",
            "codium %F",
            "/usr/bin/env A=b kate",
        ] {
            let found = |name: &str| Some(PathBuf::from("/usr/bin").join(name));
            assert!(
                linux_editor_command(&tokens(line), None, &found).is_err(),
                "{line}"
            );
        }
    }

    #[test]
    fn the_terminal_default_prefers_system_launchers() {
        let listed = ["/usr/share/applications/org.gnome.Ptyxis.desktop".to_string()];
        let both = |name: &str| Some(PathBuf::from("/usr/bin").join(name));
        assert_eq!(
            linux_terminal_default(&both, &listed).as_deref(),
            Some("/usr/bin/xdg-terminal-exec")
        );
        let debian = |name: &str| {
            (name == "x-terminal-emulator").then(|| PathBuf::from("/usr/bin/x-terminal-emulator"))
        };
        assert_eq!(
            linux_terminal_default(&debian, &listed).as_deref(),
            Some("/usr/bin/x-terminal-emulator")
        );
        assert_eq!(
            linux_terminal_default(&nowhere, &listed).as_deref(),
            Some(listed[0].as_str())
        );
        assert_eq!(linux_terminal_default(&nowhere, &[]), None);
    }

    #[test]
    fn appimage_cleanup_preserves_non_utf8_system_paths() {
        use std::os::unix::ffi::OsStringExt;

        let root = Path::new("/tmp/.mount_Silo");
        let system = OsString::from_vec(b"/opt/editor \xff/lib".to_vec());
        let mut libraries = OsString::from("/tmp/.mount_Silo/usr/lib:");
        libraries.push(&system);
        libraries.push(":/tmp/.mount_Silo-other/lib");
        let changes =
            appimage_child_environment([(OsString::from("LD_LIBRARY_PATH"), libraries)], root);
        let mut expected = system;
        expected.push(":/tmp/.mount_Silo-other/lib");
        assert_eq!(
            changes,
            [(OsString::from("LD_LIBRARY_PATH"), Some(expected))]
        );
    }

    #[test]
    fn appimage_cleanup_accepts_non_utf8_mounts_and_repeated_slashes() {
        use std::os::unix::ffi::OsStringExt;

        let root = PathBuf::from(OsString::from_vec(b"/tmp/.mount_Silo \xff///".to_vec()));
        let bundled = OsString::from_vec(b"/tmp//.mount_Silo \xff////usr/lib".to_vec());
        let changes = appimage_child_environment(
            [
                (OsString::from("APPDIR"), root.as_os_str().to_owned()),
                (OsString::from("LD_LIBRARY_PATH"), bundled),
            ],
            &root,
        );
        assert_eq!(
            changes,
            [
                (OsString::from("APPDIR"), None),
                (OsString::from("LD_LIBRARY_PATH"), None),
            ]
        );
    }

    #[test]
    fn appimage_hooks_are_undone_for_children() {
        let root = Path::new("/tmp/.mount_SiloAbC");
        let environment = [
            ("APPDIR", "/tmp/.mount_SiloAbC"),
            ("APPIMAGE", "/home/me/Silo.AppImage"),
            (
                "GSETTINGS_SCHEMA_DIR",
                "/tmp/.mount_SiloAbC//usr/share/glib-2.0/schemas",
            ),
            ("GTK_PATH", "/tmp/.mount_SiloAbC//usr/lib/gtk-3.0"),
            (
                "GDK_PIXBUF_MODULE_FILE",
                "/tmp/.mount_SiloAbC//usr/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache",
            ),
            (
                "GST_PLUGIN_SYSTEM_PATH_1_0",
                "/tmp/.mount_SiloAbC/usr/lib/gstreamer-1.0",
            ),
            (
                "XDG_DATA_DIRS",
                "/tmp/.mount_SiloAbC/usr/share:/usr/share:/usr/local/share",
            ),
            ("PATH", "/tmp/.mount_SiloAbC/usr/bin:/usr/bin:/bin"),
            ("LD_LIBRARY_PATH", "/tmp/.mount_SiloAbC/usr/lib"),
            ("HOME", "/home/me"),
            ("OTHER_MOUNT", "/tmp/.mount_SiloAbCdef/usr/lib"),
        ]
        .map(|(name, value)| (OsString::from(name), OsString::from(value)));
        let changes: std::collections::HashMap<_, _> =
            appimage_child_environment(environment, root)
                .into_iter()
                .map(|(name, value)| {
                    (
                        name.into_string().unwrap(),
                        value.map(|value| value.into_string().unwrap()),
                    )
                })
                .collect();
        for removed in [
            "APPDIR",
            "APPIMAGE",
            "GSETTINGS_SCHEMA_DIR",
            "GTK_PATH",
            "GDK_PIXBUF_MODULE_FILE",
            "GST_PLUGIN_SYSTEM_PATH_1_0",
            "LD_LIBRARY_PATH",
        ] {
            assert_eq!(changes.get(removed), Some(&None), "{removed}");
        }
        assert_eq!(
            changes["XDG_DATA_DIRS"].as_deref(),
            Some("/usr/share:/usr/local/share")
        );
        assert_eq!(changes["PATH"].as_deref(), Some("/usr/bin:/bin"));
        assert!(!changes.contains_key("HOME"));
        assert!(
            !changes.contains_key("OTHER_MOUNT"),
            "a different mount is not this AppImage"
        );
    }

    #[test]
    fn children_outside_an_appimage_keep_their_environment() {
        // The test runner is not an AppImage.
        assert!(child_environment().is_empty());
        let mut command = Command::new("/usr/bin/true");
        sanitize_child(&mut command);
        assert_eq!(command.get_envs().count(), 0);
        assert!(!tools_are_temporary());
        assert_eq!(
            stable_executable().unwrap(),
            std::env::current_exe().unwrap()
        );
    }
}
