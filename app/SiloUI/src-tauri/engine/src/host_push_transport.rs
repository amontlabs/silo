//! Private, host-authenticated SSH transport for reading a computer repository.
//! Git and Git LFS receive the same transport; neither receives GitHub credentials.
use crate::{editor, runtime::RuntimePaths};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub(crate) struct Transport {
    pub(crate) ssh_command: String,
    pub(crate) alias: String,
    config: PathBuf,
    directory: PathBuf,
}

pub(crate) fn prepare(
    paths: &RuntimePaths,
    computer: &str,
    directory: &Path,
) -> Result<Transport, String> {
    if !Path::new("/usr/bin/ssh").is_file() || !Path::new("/usr/bin/ssh-keygen").is_file() {
        return Err(
            "OpenSSH is required to transfer committed repository data from the computer.".into(),
        );
    }
    let (alias, config) = editor::prepare_private_transport(paths, computer, directory)?;
    if !alias
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
    {
        return Err("The computer SSH alias contains unsupported characters.".into());
    }
    let config_path = config.clone();
    let config = config
        .to_str()
        .ok_or("The computer SSH configuration path is not UTF-8.")?;
    Ok(Transport {
        ssh_command: format!(
            "/usr/bin/ssh -F {} -o ConnectTimeout=15 -o ClearAllForwardings=yes -o RequestTTY=no",
            shell_quote(config)?
        ),
        alias,
        config: config_path,
        directory: directory.into(),
    })
}

/// The one repository path check used before any credential is minted or
/// export runs: an absolute, normalized path strictly below `/workspace`.
pub(crate) fn valid_repository_path(path: &str) -> bool {
    path.starts_with("/workspace/")
        && !path.chars().any(char::is_control)
        && !path
            .split('/')
            .skip(1)
            .any(|part| part.is_empty() || part == "." || part == "..")
}

impl Transport {
    /// Install only into a fresh, operation-specific directory. The caller owns
    /// cleanup, including cleanup after a partial transfer.
    pub(crate) fn install_lfs_server(
        &mut self,
        binary: &Path,
        guest_directory: &str,
    ) -> Result<(), String> {
        if !guest_directory
            .strip_prefix("/tmp/silo-push-")
            .is_some_and(|suffix| {
                !suffix.is_empty()
                    && suffix
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            })
        {
            return Err("Invalid temporary computer transfer directory.".into());
        }
        let directory = shell_quote(guest_directory)?;
        let script = format!("umask 077; mkdir -m 700 {directory} && cat > {directory}/git-lfs-transfer && chmod 700 {directory}/git-lfs-transfer");
        let input = fs::File::open(binary)
            .map_err(|_| "Could not open the bundled Git LFS transfer server.")?;
        let mut child = Command::new("/usr/bin/ssh")
            .arg("-F")
            .arg(&self.config)
            .args([
                "-o",
                "ConnectTimeout=15",
                "-o",
                "ClearAllForwardings=yes",
                "-o",
                "RequestTTY=no",
                &self.alias,
                &script,
            ])
            .stdin(input)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "Could not install the Git LFS transfer server in the computer.")?;
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if crate::runtime::operation_gate::cancel_requested() {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Installing the Git LFS transfer server was cancelled.".into());
            }
            match child.try_wait() {
                Ok(Some(status)) if status.success() => break,
                Ok(Some(_)) => {
                    return Err(
                        "Could not install the Git LFS transfer server in the computer.".into(),
                    )
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(
                        "Timed out installing the Git LFS transfer server in the computer.".into(),
                    );
                }
            }
        }
        let wrapper = self.directory.join("ssh-with-lfs");
        editor::write_private(
            &wrapper,
            wrapper_script(&self.config, guest_directory)?.as_bytes(),
        )?;
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700))
            .map_err(|_| "Could not prepare the Git LFS transport.")?;
        self.ssh_command = shell_quote(wrapper.to_str().ok_or("Invalid Git LFS transport path.")?)?;
        Ok(())
    }

    pub(crate) fn repository_url(&self, path: &str) -> Result<String, String> {
        if !valid_repository_path(path) {
            return Err("Choose a repository inside /workspace.".into());
        }
        // URI encoding is separate from shell escaping. Git decodes this path
        // and quotes the upload-pack command passed to SSH itself.
        let mut encoded = String::new();
        for byte in path.bytes() {
            if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
                encoded.push(byte as char);
            } else {
                use std::fmt::Write;
                write!(&mut encoded, "%{byte:02X}").expect("writing to String cannot fail");
            }
        }
        Ok(format!("ssh://{}{encoded}", self.alias))
    }
}

fn wrapper_script(config: &Path, guest_directory: &str) -> Result<String, String> {
    let config = shell_quote(config.to_str().ok_or("Invalid SSH configuration path.")?)?;
    let prefix = shell_quote(&format!("PATH={}:$PATH ", shell_quote(guest_directory)?))?;
    // Git and Git LFS supply SSH options, hostname, then one already-quoted
    // remote command. Preserve their arguments and prepend only a PATH assignment.
    // Both callers must select SSH variant explicitly to avoid -G probing.
    Ok(format!("#!/bin/bash\nset -eu\n[ \"$#\" -ge 2 ] || exit 64\ncommand=${{@: -1}}\nexec /usr/bin/ssh -F {config} -o ConnectTimeout=15 -o ClearAllForwardings=yes -o RequestTTY=no \"${{@:1:$#-1}}\" {prefix}\"$command\"\n"))
}

fn shell_quote(value: &str) -> Result<String, String> {
    if value.chars().any(char::is_control) {
        return Err("The computer SSH configuration path contains control characters.".into());
    }
    Ok(format!("'{}'", value.replace('\'', "'\\''")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Command};

    #[test]
    fn repository_paths_are_uri_encoded_and_restricted_to_computer() {
        let transport = Transport {
            ssh_command: String::new(),
            alias: "silo-runtime-dev".into(),
            config: PathBuf::new(),
            directory: PathBuf::new(),
        };
        assert_eq!(
            transport.repository_url("/workspace/a 'b%#é").unwrap(),
            "ssh://silo-runtime-dev/workspace/a%20%27b%25%23%C3%A9"
        );
        for path in [
            "/tmp/repo",
            "/workspace/../etc",
            "/workspace/./repo",
            "/workspace//repo",
            "/workspace/repo\n",
        ] {
            assert!(transport.repository_url(path).is_err(), "{path}");
        }
    }

    #[test]
    fn ssh_command_quotes_shell_metacharacters() {
        let value = "/tmp/has 'quotes' $(echo bad) % space";
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(format!("printf '%s' {}", shell_quote(value).unwrap()))
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, value.as_bytes());
        assert!(shell_quote("/tmp/a\nb").is_err());
    }

    #[test]
    fn wrapper_preserves_ssh_arguments_and_quotes_remote_path() {
        let directory = tempfile::tempdir().unwrap();
        let wrapper = directory.path().join("wrapper");
        let script = wrapper_script(Path::new("/tmp/config 'quoted' %"), "/tmp/silo-push-test")
            .unwrap()
            .replace("exec /usr/bin/ssh ", "exec /usr/bin/printf '%s\\n' ");
        fs::write(&wrapper, script).unwrap();
        let remote = "git-lfs-transfer '/workspace/has '\\''quotes' download";
        let output = Command::new("/bin/bash")
            .arg(&wrapper)
            .args(["-o", "SendEnv=GIT_PROTOCOL", "root@silo-test", remote])
            .output()
            .unwrap();
        assert!(output.status.success());
        let output = String::from_utf8(output.stdout).unwrap();
        let lines: Vec<_> = output.lines().collect();
        assert_eq!(lines[1], "/tmp/config 'quoted' %");
        assert_eq!(
            &lines[lines.len() - 4..lines.len() - 1],
            &["-o", "SendEnv=GIT_PROTOCOL", "root@silo-test"]
        );
        assert_eq!(
            lines.last().unwrap(),
            &format!("PATH='/tmp/silo-push-test':$PATH {remote}")
        );
    }

    #[test]
    fn private_configuration_pins_identity_and_does_not_install_user_config() {
        let directory = tempfile::tempdir().unwrap();
        let paths = RuntimePaths {
            guest_image: PathBuf::from("/unused/guest-image"),
            executable: directory.path().join("msb"),
            home: directory.path().join("runtime"),
            storage_home: None,
            library: directory.path().join("msb"),
            metadata: directory.path().join("computers.json"),
            volumes: directory.path().join("volumes"),
        };
        crate::working_account::test_runtime(&paths.executable);
        let private = directory.path().join("operation with 'quotes' and %");
        let transport = prepare(&paths, "dev", &private).unwrap();
        assert!(!paths.home.join("ssh/dev.conf").exists());
        assert!(!directory.path().join(".ssh").exists());
        assert_eq!(
            fs::metadata(private.join("ssh_config"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let output = Command::new("/usr/bin/ssh")
            .arg("-G")
            .arg("-F")
            .arg(private.join("ssh_config"))
            .arg(&transport.alias)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let config = String::from_utf8(output.stdout).unwrap();
        for required in [
            "user silo\n",
            "stricthostkeychecking true",
            "identitiesonly yes",
            "identityagent none",
            "forwardagent no",
            "batchmode yes",
            "--no-start",
            "--stdio",
        ] {
            assert!(config.contains(required), "missing {required}");
        }
        let initial = fs::read(paths.home.join("ssh/authorized_keys")).unwrap();
        prepare(&paths, "dev", &private).unwrap();
        assert_eq!(
            fs::read(paths.home.join("ssh/authorized_keys")).unwrap(),
            initial
        );
    }
}
