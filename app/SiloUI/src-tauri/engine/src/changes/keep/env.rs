//! What a keep needs from the world: commands in two guests, files between them,
//! checkpoints of main and the stored manifests. Tests supply fakes; the engine supplies
//! [`super::guest_env::GuestEnv`].

use std::time::Duration;

use super::super::diff::Diff;
use super::super::manifest::Manifest;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Linux,
    Macos,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunAs {
    Root,
    Account,
}

/// One guest command. `tag` names the operation (fakes dispatch on it); `args` reach the
/// script as `A_<name>` environment variables, so values are never quoted into shell text.
#[derive(Clone, Debug)]
pub struct GuestCmd {
    pub tag: &'static str,
    pub args: Vec<(&'static str, String)>,
    pub script: String,
    pub stdin: Option<Vec<u8>>,
    pub timeout: Duration,
}

impl GuestCmd {
    pub fn new(tag: &'static str, script: impl Into<String>) -> Self {
        Self {
            tag,
            args: Vec::new(),
            script: script.into(),
            stdin: None,
            timeout: Duration::from_secs(170),
        }
    }

    pub fn arg(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.args.push((name, value.into()));
        self
    }

    pub fn stdin(mut self, data: impl Into<Vec<u8>>) -> Self {
        self.stdin = Some(data.into());
        self
    }

    pub fn get(&self, name: &str) -> &str {
        self.args
            .iter()
            .find(|(key, _)| *key == name)
            .map_or("", |(_, value)| value)
    }
}

#[derive(Clone, Debug, Default)]
pub struct ExecOut {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl ExecOut {
    pub fn ok(stdout: impl Into<Vec<u8>>) -> Self {
        Self {
            code: 0,
            stdout: stdout.into(),
            stderr: Vec::new(),
        }
    }

    pub fn failed(code: i32, stderr: &str) -> Self {
        Self {
            code,
            stdout: Vec::new(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    /// The output when the command succeeded, else its error text.
    pub fn success(self, what: &str) -> Result<String, String> {
        if self.code == 0 {
            Ok(String::from_utf8_lossy(&self.stdout).into_owned())
        } else {
            let err = String::from_utf8_lossy(&self.stderr);
            let err = err.trim();
            let skip = err.chars().count().saturating_sub(400);
            let tail: String = err.chars().skip(skip).collect();
            Err(format!("{what} failed (exit {}): {tail}", self.code))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CheckpointRef {
    pub id: String,
    pub name: String,
    pub created_at: String,
}

pub trait KeepEnv: Send + Sync {
    fn os(&self, computer: &str) -> Result<Os, String>;
    fn run(&self, computer: &str, who: RunAs, cmd: &GuestCmd) -> Result<ExecOut, String>;

    /// File bytes and the permission bits (octal text).
    fn read(&self, computer: &str, path: &str) -> Result<(Vec<u8>, String), String> {
        let os = self.os(computer)?;
        let mode = self
            .run(computer, RunAs::Root, &super::scripts::file_mode(os, path))?
            .success("Reading a file mode")?;
        let data = self.run(computer, RunAs::Root, &super::scripts::file_read(path))?;
        if data.code != 0 {
            return Err(data.success("Reading a file").unwrap_err());
        }
        Ok((data.stdout, mode.trim().to_owned()))
    }

    /// Writes next to the target and renames it into place, keeping the owner of what is
    /// there (or of the folder).
    fn write(&self, computer: &str, path: &str, data: &[u8], mode: &str) -> Result<(), String> {
        let os = self.os(computer)?;
        self.run(
            computer,
            RunAs::Root,
            &super::scripts::file_write(os, path, mode, data),
        )?
        .success("Writing a file")
        .map(|_| ())
    }

    /// Saves a before-keep checkpoint of `computer` and returns it.
    fn checkpoint_before_keep(&self, computer: &str, name: &str) -> Result<CheckpointRef, String>;
    /// This computer's before-keep checkpoints, oldest first.
    fn before_keep_checkpoints(&self, computer: &str) -> Result<Vec<CheckpointRef>, String>;
    fn restore(&self, computer: &str, checkpoint_id: &str) -> Result<(), String>;
    /// Progress lines for the job.
    fn note(&self, _line: &str) {}
}

/// Manifests recorded by `changes::collect`.
pub trait ManifestStore: Send + Sync {
    /// The fork's state when it was forked.
    fn fork_time(&self, fork: &str) -> Result<Manifest, String>;
    /// Looks at `computer` now.
    fn capture(&self, computer: &str) -> Result<Manifest, String>;
    /// The no-op baseline of the fork's source, when one is cached.
    fn baseline(&self, _fork: &str) -> Option<Diff> {
        None
    }
    /// The text of `user`'s crontab at fork time, when the collector recorded it. Without it
    /// a crontab that main changed since is merged by adding the fork's new lines only.
    fn fork_time_crontab(&self, _fork: &str, _user: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
    /// The computer the fork was made from.
    fn source(&self, fork: &str) -> Result<Option<String>, String>;
}
