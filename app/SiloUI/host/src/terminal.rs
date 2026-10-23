//! Launching a terminal on the host.
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

pub fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub fn command_file(command: &str) -> Result<std::path::PathBuf, String> {
    let mut file = tempfile::Builder::new()
        .prefix("silo-terminal-")
        .suffix(".command")
        .tempfile()
        .map_err(|_| "Could not prepare terminal command.")?;
    file.as_file()
        .set_permissions(fs::Permissions::from_mode(0o700))
        .map_err(|_| "Could not protect terminal command.")?;
    writeln!(file, "#!/bin/sh\nrm -f -- \"$0\"\nexec {command}")
        .map_err(|_| "Could not write terminal command.")?;
    file.into_temp_path()
        .keep()
        .map_err(|_| "Could not keep terminal command.".into())
}
pub fn launch(mut command: Command) -> Result<(), String> {
    // Under an AppImage the terminal gets the system environment (G-24).
    let mut child = crate::applications::launch::sanitize_child(&mut command)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "The selected terminal could not be opened.")?;
    // GUI launchers may stay alive for the lifetime of their window. Detect early
    // failures, then reap asynchronously without blocking the Silo action.
    std::thread::sleep(Duration::from_millis(150));
    match child
        .try_wait()
        .map_err(|_| "Could not check terminal launch.")?
    {
        Some(status) if !status.success() => {
            Err("The selected terminal rejected the command. Check its installation.".into())
        }
        Some(_) => Ok(()),
        None => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            Ok(())
        }
    }
}
pub fn linux_arguments(executable: &Path) -> Result<&'static [&'static str], String> {
    match executable.file_name().and_then(|n| n.to_str()) {
        // xdg-terminal-exec runs its arguments in the preferred terminal;
        // Debian's x-terminal-emulator must accept -e (G-25).
        Some("xdg-terminal-exec") => Ok(&[]),
        Some("x-terminal-emulator") => Ok(&["-e"]),
        Some("gnome-terminal" | "kgx" | "ptyxis") => Ok(&["--"]),
        Some("wezterm") => Ok(&["start", "--"]),
        Some("xfce4-terminal") => Ok(&["--execute"]),
        Some("ghostty" | "konsole" | "alacritty" | "kitty" | "xterm" | "tilix") => Ok(&["-e"]),
        _ => Err("This terminal does not have a supported command launcher. Choose another terminal in Settings.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_arguments_stay_literal() {
        let value = "spaces ' quotes $(touch /tmp/never) `whoami`";
        let output = Command::new("/bin/sh")
            .args(["-c", &format!("printf %s {}", quote(value))])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8(output.stdout).unwrap(), value);
    }

    #[test]
    fn private_script_executes_and_removes_itself() {
        let path = command_file("/bin/echo silo-terminal-test").unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let output = Command::new(&path).output().unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"silo-terminal-test\n");
        assert!(!path.exists());
    }

    #[test]
    fn terminal_adapters_use_argument_boundaries_and_reject_unknown_apps() {
        assert_eq!(
            linux_arguments(Path::new("/bin/gnome-terminal")).unwrap(),
            ["--"]
        );
        assert_eq!(
            linux_arguments(Path::new("/bin/wezterm")).unwrap(),
            ["start", "--"]
        );
        assert_eq!(
            linux_arguments(Path::new("/usr/bin/ptyxis")).unwrap(),
            ["--"]
        );
        assert!(linux_arguments(Path::new("/usr/bin/xdg-terminal-exec"))
            .unwrap()
            .is_empty());
        assert_eq!(
            linux_arguments(Path::new("/usr/bin/x-terminal-emulator")).unwrap(),
            ["-e"]
        );
        assert!(linux_arguments(Path::new("/bin/unknown")).is_err());
    }
}
