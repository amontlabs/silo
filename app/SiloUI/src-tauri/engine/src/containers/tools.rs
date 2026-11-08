//! The programs a container computer needs: Silo's own helpers beside the engine and the
//! system's crun, skopeo, umoci and systemd-run. Missing system programs are reported through
//! the host prerequisite check (`silo host init` names the same ones).

use crate::runtime::{RuntimeError, RuntimePaths};
use std::path::{Path, PathBuf};

pub const CTR: &str = "silo-ctr";
pub const INIT: &str = "silo-init";

#[derive(Clone, Debug)]
pub struct Tools {
    pub ctr: PathBuf,
    pub init: PathBuf,
    pub gateway: PathBuf,
    pub crun: PathBuf,
    pub skopeo: PathBuf,
    pub umoci: PathBuf,
    pub systemd_run: PathBuf,
}

pub fn find_in_path(name: &str, path: &std::ffi::OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|directory| directory.join(name))
        .find(|candidate| {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(candidate)
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
}

impl Tools {
    /// Locates every program, naming the first one that is missing.
    pub fn locate(paths: &RuntimePaths) -> Result<Self, RuntimeError> {
        let beside = |name: &str| -> Result<PathBuf, RuntimeError> {
            let path = paths.executable.with_file_name(name);
            if path.is_file() {
                Ok(path)
            } else {
                Err(RuntimeError::Unavailable(format!(
                    "{name} is missing from this Silo engine. Reinstall Silo."
                )))
            }
        };
        let system_path = std::env::var_os("PATH").unwrap_or_default();
        let system = |name: &str| -> Result<PathBuf, RuntimeError> {
            find_in_path(name, &system_path).ok_or_else(|| {
                RuntimeError::Unavailable(format!(
                    "{name} is not installed on this host. Run silo host init to see what container computers need."
                ))
            })
        };
        Ok(Self {
            ctr: beside(CTR)?,
            init: beside(INIT)?,
            gateway: beside(crate::computer_gateway::GATEWAY_BINARY)?,
            crun: system("crun")?,
            skopeo: system("skopeo")?,
            umoci: system("umoci")?,
            systemd_run: system("systemd-run")?,
        })
    }
}

/// Runs a short program to completion and returns its standard output, or its error text.
pub fn run_checked(program: &Path, args: &[&std::ffi::OsStr]) -> Result<String, String> {
    let output = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| format!("{} could not start: {error}", program.display()))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let text = String::from_utf8_lossy(&output.stderr);
        let last = text
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("");
        Err(format!(
            "{} failed ({}): {last}",
            program
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("program"),
            output.status
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programs_are_found_on_the_given_path_only_when_executable() {
        let directory = tempfile::tempdir().unwrap();
        let program = directory.path().join("crun");
        std::fs::write(&program, "#!/bin/sh\n").unwrap();
        let path = std::env::join_paths([directory.path()]).unwrap();
        assert_eq!(find_in_path("crun", &path), None);
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(find_in_path("crun", &path), Some(program));
        assert_eq!(find_in_path("skopeo", &path), None);
    }

    #[test]
    fn a_failing_tool_reports_its_last_error_line() {
        let error = run_checked(
            Path::new("/bin/sh"),
            &["-c".as_ref(), "echo a >&2; echo b >&2; exit 3".as_ref()],
        )
        .unwrap_err();
        assert!(
            error.contains("sh failed") && error.ends_with(": b"),
            "{error}"
        );
    }
}
