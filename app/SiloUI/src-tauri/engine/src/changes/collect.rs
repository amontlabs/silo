//! Captures a computer's state manifest (format 2) through the engine's guest exec path.
//!
//! `guest.exec` returns at most 768 KiB and runs at most 180 s, and a whole-computer manifest
//! can exceed both. The collector therefore runs detached in the guest, writes the manifest to
//! a guest temp file and gzips it, and the engine polls for its end with several bounded calls
//! and reads the file back in chunks. A second script attributes changed paths to the package
//! that installed them (`OWN` records).

use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};
use silo_api::jobs::ErrorCode;

use crate::guest::{
    self, ExecRequest, GuestError, GuestUser, Target, MAX_BOUNDED_TIMEOUT_MS, MAX_OUTPUT_BYTES,
    MAX_RESPONSE_RAW_BYTES,
};

/// The guest-side collector, shipped inside the engine.
pub const COLLECT_SCRIPT: &str = include_str!("collect.sh");
/// The guest-side owner query.
pub const OWNER_SCRIPT: &str = include_str!("owners.sh");

/// The most changed paths asked about in one owner query.
pub const MAX_OWNER_PATHS: usize = 5000;

/// How a command ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecOut {
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// A range of a guest file and the file's size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileChunk {
    pub data: Vec<u8>,
    pub size: u64,
}

/// The two guest operations capture needs. `TargetRunner` is the engine's own guest path; a
/// test supplies a fake.
pub trait GuestRunner: Send + Sync {
    fn is_macos(&self) -> bool;
    /// Runs `argv` to completion (at most 180 s, 768 KiB of output).
    fn exec(
        &self,
        argv: &[String],
        stdin: Option<Vec<u8>>,
        user: GuestUser,
        timeout: Duration,
    ) -> Result<ExecOut, GuestError>;
    /// At most `MAX_RESPONSE_RAW_BYTES` bytes from `offset`.
    fn read(&self, path: &str, offset: u64, length: u64) -> Result<FileChunk, GuestError>;
}

/// Runs on a resolved computer through `guest::run_bounded` and `guest::read`.
pub struct TargetRunner(pub Target);

impl GuestRunner for TargetRunner {
    fn is_macos(&self) -> bool {
        self.0.is_macos()
    }

    fn exec(
        &self,
        argv: &[String],
        stdin: Option<Vec<u8>>,
        user: GuestUser,
        timeout: Duration,
    ) -> Result<ExecOut, GuestError> {
        let output = guest::run_bounded(
            &self.0,
            &ExecRequest {
                argv: argv.to_vec(),
                cwd: None,
                env: Vec::new(),
                user,
                stdin,
                timeout,
                max_stdout: MAX_OUTPUT_BYTES,
            },
        )?;
        Ok(ExecOut {
            exit_code: output.exit_code,
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    fn read(&self, path: &str, offset: u64, length: u64) -> Result<FileChunk, GuestError> {
        let read = guest::read(&self.0, path, Some(offset), Some(length))?;
        Ok(FileChunk {
            data: read.data,
            size: read.size,
        })
    }
}

/// Limits of one capture.
#[derive(Clone, Debug)]
pub struct Options {
    /// The whole capture, summed over its bounded calls.
    pub budget: Duration,
    /// How long one polling call waits for the collector (at most 150 s).
    pub poll: Duration,
    /// Bytes per file read.
    pub chunk: u64,
    /// The largest manifest accepted once decompressed.
    pub max_manifest_bytes: u64,
    /// The largest compressed manifest read back from the computer.
    pub max_compressed_bytes: u64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            budget: Duration::from_secs(20 * 60),
            poll: Duration::from_secs(60),
            chunk: MAX_RESPONSE_RAW_BYTES as u64,
            max_manifest_bytes: 512 * 1024 * 1024,
            max_compressed_bytes: 128 * 1024 * 1024,
        }
    }
}

const SHORT_CALL: Duration = Duration::from_secs(60);

/// Starts the collector detached in a process group of its own: the script arrives on stdin,
/// runs under `bash`, and ends by writing `DONE` (its exit status) after the gzip and its
/// digest. One collector runs per computer: the lock file names its process group, and a
/// second start while that one lives answers `busy`.
const START: &str = r#"d=$1
lock=/tmp/silo-collect.lock
if ! ( set -C; echo $$ > "$lock" ) 2>/dev/null; then
  old=$(cat "$lock" 2>/dev/null)
  if [ -n "$old" ] && kill -0 "$old" 2>/dev/null; then echo busy; exit 0; fi
  rm -f "$lock"
  ( set -C; echo $$ > "$lock" ) 2>/dev/null || { echo busy; exit 0; }
fi
umask 022
mkdir -p "$d" && cat > "$d/collect.sh" || { rm -f "$lock"; exit 1; }
set -m
nohup sh -c 'cd "$1" || exit 1
bash collect.sh > m.tsv 2> err.txt < /dev/null
rc=$?
gzip -f m.tsv
chmod 644 m.tsv.gz err.txt
( sha256sum m.tsv.gz 2>/dev/null || shasum -a 256 m.tsv.gz ) | cut -d" " -f1 > sum
echo $rc > DONE' sh "$d" < /dev/null > /dev/null 2>&1 &
echo $! > "$d/pid"
echo $! > "$lock"
echo started"#;

/// Ends the collector of folder `$1` (its whole process group) when it still runs, then removes
/// the folder and the lock if the lock is its own. Safe to run after a normal end.
const CLEAN: &str = r#"d=$1
lock=/tmp/silo-collect.lock
p=$(cat "$d/pid" 2>/dev/null)
case "$p" in ''|*[!0-9]*) p= ;; esac
if [ -n "$p" ] && kill -0 "$p" 2>/dev/null; then
  kill -TERM -- "-$p" 2>/dev/null; kill -TERM "$p" 2>/dev/null
  i=0; while kill -0 "$p" 2>/dev/null && [ "$i" -lt 5 ]; do sleep 1; i=$((i+1)); done
  kill -KILL -- "-$p" 2>/dev/null; kill -KILL "$p" 2>/dev/null
fi
if [ -n "$p" ] && [ "$(cat "$lock" 2>/dev/null)" = "$p" ]; then rm -f "$lock"; fi
rm -rf "$d""#;

/// Waits up to `$2` seconds for the collector; prints `done <rc> <sha256>`, `running` or `gone`
/// (the collector's process ended without finishing).
const POLL: &str = r#"d=$1; n=$2; i=0
while [ ! -f "$d/DONE" ] && [ "$i" -lt "$n" ]; do
  if [ -f "$d/pid" ] && ! kill -0 "$(cat "$d/pid")" 2>/dev/null && [ ! -f "$d/DONE" ]; then echo gone; exit 0; fi
  sleep 1; i=$((i+1))
done
if [ -f "$d/DONE" ]; then echo "done $(cat "$d/DONE") $(cat "$d/sum" 2>/dev/null)"; else echo running; fi"#;

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

fn exec_user(runner: &dyn GuestRunner) -> GuestUser {
    if runner.is_macos() {
        GuestUser::Account
    } else {
        GuestUser::Root
    }
}

fn text(output: &ExecOut) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn failed(what: &str, output: &ExecOut) -> GuestError {
    let detail = String::from_utf8_lossy(&output.stderr);
    GuestError::internal(format!(
        "{what} failed ({}){}",
        output
            .exit_code
            .map_or("no exit status".to_owned(), |code| format!("exit {code}")),
        if detail.trim().is_empty() {
            String::new()
        } else {
            format!(": {}", detail.trim())
        }
    ))
}

/// Captures the manifest of the computer as text, without owner records.
pub fn capture(runner: &dyn GuestRunner, options: &Options) -> Result<String, GuestError> {
    let directory = format!("/tmp/silo-collect-{}", uuid::Uuid::new_v4().simple());
    let user = exec_user(runner);
    let outcome = run_collector(runner, &directory, user, options);
    // Neither the collector nor its temporary folder outlives the capture, whichever way it
    // ended: one that is still running (a timeout) is stopped here.
    let _ = runner.exec(
        &argv(&["sh", "-c", CLEAN, "sh", &directory]),
        None,
        user,
        Duration::from_secs(30),
    );
    outcome
}

fn run_collector(
    runner: &dyn GuestRunner,
    directory: &str,
    user: GuestUser,
    options: &Options,
) -> Result<String, GuestError> {
    let started = Instant::now();
    let launch = runner.exec(
        &argv(&["sh", "-c", START, "sh", directory]),
        Some(COLLECT_SCRIPT.as_bytes().to_vec()),
        user,
        SHORT_CALL,
    )?;
    if launch.exit_code == Some(0) && text(&launch) == "busy" {
        return Err(GuestError::new(
            ErrorCode::Busy,
            "Another capture of this computer is still running.",
        ));
    }
    if launch.exit_code != Some(0) || text(&launch) != "started" {
        return Err(failed("Starting the collector", &launch));
    }
    let digest = loop {
        let left = options.budget.saturating_sub(started.elapsed());
        if left.is_zero() {
            return Err(GuestError::new(
                ErrorCode::TimedOut,
                "The computer took too long to describe itself.",
            ));
        }
        let wait = options
            .poll
            .min(left)
            .min(Duration::from_secs(MAX_BOUNDED_TIMEOUT_MS / 1000 - 30));
        let seconds = wait.as_secs().max(1).to_string();
        let poll = runner.exec(
            &argv(&["sh", "-c", POLL, "sh", directory, &seconds]),
            None,
            user,
            wait + Duration::from_secs(25),
        )?;
        if poll.exit_code != Some(0) {
            return Err(failed("Waiting for the collector", &poll));
        }
        let answer = text(&poll);
        let mut words = answer.split_whitespace();
        match words.next() {
            Some("running") => continue,
            Some("gone") => {
                return Err(GuestError::internal(
                    "The collector stopped before it finished.",
                ))
            }
            Some("done") => {
                let status = words.next().unwrap_or("");
                if status != "0" {
                    return Err(collector_failure(runner, directory, user, status));
                }
                break words.next().unwrap_or("").to_owned();
            }
            _ => return Err(failed("Waiting for the collector", &poll)),
        }
    };
    let compressed = read_bounded(
        runner,
        &format!("{directory}/m.tsv.gz"),
        options.chunk,
        options.max_compressed_bytes,
        started + options.budget,
    )?;
    if !digest.is_empty() {
        let actual = hex(&Sha256::digest(&compressed));
        if actual != digest {
            return Err(GuestError::internal(
                "The manifest changed while it was read back.",
            ));
        }
    }
    let mut manifest = String::new();
    flate2::read::GzDecoder::new(compressed.as_slice())
        .take(options.max_manifest_bytes + 1)
        .read_to_string(&mut manifest)
        .map_err(|error| GuestError::internal(format!("The manifest is not readable: {error}")))?;
    if manifest.len() as u64 > options.max_manifest_bytes {
        return Err(GuestError::new(
            ErrorCode::OutputTooLarge,
            "The manifest is larger than the engine accepts.",
        ));
    }
    if !manifest.starts_with("META\tformat\t2\n") {
        return Err(GuestError::internal(
            "The collector did not produce a format 2 manifest.",
        ));
    }
    Ok(manifest)
}

fn collector_failure(
    runner: &dyn GuestRunner,
    directory: &str,
    user: GuestUser,
    status: &str,
) -> GuestError {
    let tail = runner
        .exec(
            &argv(&["tail", "-c", "1000", &format!("{directory}/err.txt")]),
            None,
            user,
            Duration::from_secs(30),
        )
        .map(|output| text(&output))
        .unwrap_or_default();
    GuestError::internal(format!(
        "The collector ended with status {status}{}",
        if tail.is_empty() {
            String::new()
        } else {
            format!(": {tail}")
        }
    ))
}

/// Reads a whole guest file in chunks that each fit one response, refusing a file larger than
/// `max` before reading it and giving up at `deadline`.
pub fn read_bounded(
    runner: &dyn GuestRunner,
    path: &str,
    chunk: u64,
    max: u64,
    deadline: Instant,
) -> Result<Vec<u8>, GuestError> {
    let mut bytes = Vec::new();
    loop {
        if Instant::now() >= deadline {
            return Err(GuestError::new(
                ErrorCode::TimedOut,
                "The computer took too long to describe itself.",
            ));
        }
        let offset = bytes.len() as u64;
        let part = runner.read(path, offset, chunk)?;
        if part.size > max {
            return Err(GuestError::new(
                ErrorCode::OutputTooLarge,
                "The manifest is larger than the engine accepts.",
            ));
        }
        bytes.extend_from_slice(&part.data);
        if bytes.len() as u64 >= part.size {
            return Ok(bytes);
        }
        if part.data.is_empty() {
            return Err(GuestError::internal(
                "The computer returned no data before the end of the file.",
            ));
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

// MARK: Changed paths and owners

fn unescape(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    let mut chars = field.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// File-like records of a manifest: absolute path to the rest of the record.
fn file_records(manifest: &str) -> BTreeMap<String, String> {
    let mut records = BTreeMap::new();
    for line in manifest.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        match fields.as_slice() {
            ["F" | "L", path, rest @ ..] if !rest.is_empty() => {
                records.insert(unescape(path), rest.join("\t"));
            }
            ["GIG", repo, path, rest @ ..] if !rest.is_empty() => {
                records.insert(
                    format!("{}/{}", unescape(repo), unescape(path)),
                    rest.join("\t"),
                );
            }
            _ => {}
        }
    }
    records
}

/// The paths in `current` that are new or differ from `base`, in order, at most
/// `MAX_OWNER_PATHS`. Paths with a tab or newline are left out (they cannot be asked about).
pub fn changed_paths(base: &str, current: &str) -> Vec<String> {
    let before = file_records(base);
    file_records(current)
        .into_iter()
        .filter(|(path, record)| before.get(path) != Some(record))
        .map(|(path, _)| path)
        .filter(|path| !path.contains(['\t', '\n']))
        .take(MAX_OWNER_PATHS)
        .collect()
}

/// The `OWN` lines the guest reports for `paths` (empty when there are none).
pub fn owner_records(runner: &dyn GuestRunner, paths: &[String]) -> Result<String, GuestError> {
    if paths.is_empty() {
        return Ok(String::new());
    }
    let mut stdin = paths.join("\n").into_bytes();
    stdin.push(b'\n');
    let output = runner.exec(
        &argv(&["bash", "-c", OWNER_SCRIPT]),
        Some(stdin),
        exec_user(runner),
        Duration::from_secs(150),
    )?;
    if output.exit_code != Some(0) {
        return Err(failed("The owner query", &output));
    }
    let lines: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.starts_with("OWN\t") && line.split('\t').count() == 4)
        .map(str::to_owned)
        .collect();
    Ok(lines.join("\n"))
}

/// `current` with the owner records of what changed since `base` appended. A failed owner
/// query leaves the manifest as it was: attribution is a refinement, not a requirement.
pub fn with_owners(runner: &dyn GuestRunner, base: &str, mut current: String) -> String {
    let paths = changed_paths(base, &current);
    match owner_records(runner, &paths) {
        Ok(records) if !records.is_empty() => {
            if !current.ends_with('\n') {
                current.push('\n');
            }
            current.push_str(&records);
            current.push('\n');
        }
        _ => {}
    }
    current
}

/// The paths of `OWN` records of a manifest, for tests and callers that need the set.
pub fn owned_paths(manifest: &str) -> BTreeSet<String> {
    manifest
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            matches!(fields.as_slice(), ["OWN", _, _, _]).then(|| unescape(fields[3]))
        })
        .collect()
}

#[cfg(test)]
mod tests;
