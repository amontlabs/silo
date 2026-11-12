//! What a keep needs from the world: commands in two guests, files between them,
//! checkpoints of main and the stored manifests. Tests supply fakes; the engine supplies
//! [`super::guest_env::GuestEnv`].

use std::{any::Any, collections::BTreeMap, time::Duration};

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
    /// `full` when the checkpoint holds the computer's memory (it was running), `disk` when
    /// not. Empty for a record from before this was kept.
    #[serde(default)]
    pub scope: String,
}

impl CheckpointRef {
    pub fn has_memory(&self) -> bool {
        self.scope == "full"
    }
}

/// What a path holds in a guest, looked at without following links.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Live {
    Missing,
    File {
        sha: String,
        mode: String,
    },
    Link(String),
    Dir,
    /// Present but not readable: never equal to anything.
    Unreadable,
}

/// Parses the NUL-delimited records of the `inspect` command: path, kind, mode, value.
pub fn parse_inspect(bytes: &[u8]) -> BTreeMap<String, Live> {
    let fields: Vec<&[u8]> = bytes.split(|b| *b == 0).collect();
    let mut out = BTreeMap::new();
    for record in fields.chunks_exact(4) {
        let text = |i: usize| String::from_utf8_lossy(record[i]).into_owned();
        let live = match text(1).as_str() {
            "F" => Live::File {
                sha: text(3),
                mode: text(2),
            },
            "L" => Live::Link(text(3)),
            "D" => Live::Dir,
            "X" => Live::Missing,
            _ => Live::Unreadable,
        };
        out.insert(text(0), live);
    }
    out
}

pub trait KeepEnv: Send + Sync {
    fn os(&self, computer: &str) -> Result<Os, String>;
    fn run(&self, computer: &str, who: RunAs, cmd: &GuestCmd) -> Result<ExecOut, String>;

    /// What each path holds, without following links. Every path is answered.
    fn inspect(&self, computer: &str, paths: &[String]) -> Result<BTreeMap<String, Live>, String> {
        if paths.is_empty() {
            return Ok(BTreeMap::new());
        }
        let os = self.os(computer)?;
        let out = self
            .run(computer, RunAs::Root, &super::scripts::inspect(os, paths))?
            .code_ok("Looking at files")?;
        let mut found = parse_inspect(&out);
        for path in paths {
            found.entry(path.clone()).or_insert(Live::Unreadable);
        }
        Ok(found)
    }

    /// The bytes of a regular file (never through a link).
    fn read(&self, computer: &str, path: &str) -> Result<Vec<u8>, String> {
        let out = self.run(computer, RunAs::Root, &super::scripts::file_read(path))?;
        if out.code != 0 {
            return Err(out.success("Reading a file").unwrap_err());
        }
        Ok(out.stdout)
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
    fn restore(&self, computer: &str, checkpoint_id: &str) -> Result<(), String>;
    /// Starts `computer` after a restore. A restore leaves the computer stopped; the start
    /// resumes it from the checkpoint's memory when the checkpoint holds it.
    fn start(&self, _computer: &str) -> Result<(), String> {
        Ok(())
    }
    /// The name a person knows `computer` by, for the words of a message; the id when none is known.
    fn name_of(&self, computer: &str) -> String {
        computer.to_owned()
    }
    /// Called after a keep into `computer` succeeded, with its lane released.
    fn after_kept(&self, _computer: &str) {}
    /// Holds the engine's operation lane for `computer` until the returned value is dropped,
    /// so no restore, stop, fork or delete of it interleaves with the keep.
    fn hold(&self, _computer: &str, _label: &str) -> Result<Box<dyn Any>, String> {
        Ok(Box::new(()))
    }
    /// Puts main back from an interrupted keep's checkpoint. `held` is called once main's
    /// lane is held (or, when it cannot be, just before the restore), so the caller knows
    /// nothing else can start on it meanwhile.
    fn recover(&self, computer: &str, checkpoint_id: &str, held: &dyn Fn()) -> Result<(), String> {
        held();
        self.restore(computer, checkpoint_id)
    }
    /// Progress lines for the job.
    fn note(&self, _line: &str) {}
}

impl ExecOut {
    /// The raw output when the command succeeded.
    pub fn code_ok(self, what: &str) -> Result<Vec<u8>, String> {
        if self.code == 0 {
            Ok(self.stdout)
        } else {
            self.success(what).map(|_| Vec::new())
        }
    }
}

/// Manifests recorded by `changes::collect`, as text.
pub trait ManifestStore: Send + Sync {
    /// The fork's state when it was forked.
    fn fork_time(&self, fork: &str) -> Result<String, String>;
    /// Looks at `computer` now.
    fn capture(&self, computer: &str) -> Result<String, String>;
    /// The no-op baseline of the fork's source: its base and idle manifests, when cached.
    fn baseline(&self, _fork: &str) -> Option<(String, String)> {
        None
    }
    /// The computer (or checkpoint) the fork was made from.
    fn source(&self, fork: &str) -> Result<Option<String>, String>;
}
