//! The `silo` program Silo puts into a computer. A Linux computer gets the build the engine ships
//! (`resources/guest-support/silo/`, staged by `npm run runtime:prepare` with a manifest holding
//! its SHA-256); a macOS computer gets the device's own `silo`, which is already signed. The file
//! is read once and its bytes are checked against the checksum the push is verified with, so what
//! is sent is what was checked.

use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Where the shipped Linux program is, under the engine's resources.
pub const RESOURCE_DIRECTORY: &str = "guest-support/silo";
const MAX_MANIFEST_BYTES: u64 = 16 * 1024;
/// The largest program that is sent.
pub const MAX_PROGRAM_BYTES: u64 = 64 * 1024 * 1024;
const MANIFEST_SCHEMA: u32 = 1;

/// The program for a computer, found but not yet read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Located {
    /// A file, with the SHA-256 it must have when the manifest gives one.
    File {
        path: PathBuf,
        expected_sha256: Option<String>,
    },
    /// This build has no program for the computer; the reason is shown as it is.
    Unavailable(String),
}

/// A program read and checked, ready to be sent.
#[derive(Debug)]
pub struct Payload {
    pub sha256: String,
    pub bytes: Vec<u8>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    schema_version: u32,
    available: bool,
    #[serde(default)]
    architecture: String,
    #[serde(default)]
    binary_sha256: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    reason: String,
}

/// The guest architecture of a Linux computer on this device: virtual machines run the host's.
pub fn host_architecture() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        other => other,
    }
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

const UNAVAILABLE_BUILD: &str =
    "This build of Silo has no Linux program for Silo's tools, so they cannot be installed in Linux computers by this build.";

/// The program for a Linux computer, from the engine's `resources` folder.
pub fn linux(resources: &Path, architecture: &str) -> Located {
    let directory = resources.join(RESOURCE_DIRECTORY);
    let manifest = match read_small(&directory.join("manifest.json"), MAX_MANIFEST_BYTES)
        .and_then(|bytes| serde_json::from_slice::<Manifest>(&bytes).map_err(|e| e.to_string()))
    {
        Ok(manifest) if manifest.schema_version == MANIFEST_SCHEMA => manifest,
        _ => return Located::Unavailable(UNAVAILABLE_BUILD.into()),
    };
    if !manifest.available {
        return Located::Unavailable(if manifest.reason.trim().is_empty() {
            UNAVAILABLE_BUILD.into()
        } else {
            manifest.reason.chars().take(400).collect()
        });
    }
    if manifest.architecture != architecture {
        return Located::Unavailable(format!(
            "This build's program for Silo's tools is for {} computers, not {architecture}.",
            manifest.architecture
        ));
    }
    if !is_sha256(&manifest.binary_sha256)
        || manifest.size == 0
        || manifest.size > MAX_PROGRAM_BYTES
    {
        return Located::Unavailable(UNAVAILABLE_BUILD.into());
    }
    Located::File {
        path: directory.join("silo"),
        expected_sha256: Some(manifest.binary_sha256),
    }
}

/// The program for a macOS computer: the device's own `silo`, beside the engine's tools.
pub fn device(tools_directory: Option<&Path>) -> Located {
    match tools_directory {
        Some(directory) => Located::File {
            path: directory.join("silo"),
            expected_sha256: None,
        },
        None => Located::Unavailable(
            "Silo could not find its own silo program to copy into a macOS computer.".into(),
        ),
    }
}

fn read_small(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let file = open_regular(path)?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("too large".into());
    }
    Ok(bytes)
}

fn open_regular(path: &Path) -> Result<fs::File, String> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    Ok(file)
}

impl Located {
    /// What a computer of this build can be given, without reading the file: its SHA-256 when the
    /// manifest names one, or `None` for the device's own program, which is hashed when read.
    pub fn unavailable_reason(&self) -> Option<&str> {
        match self {
            Self::Unavailable(reason) => Some(reason),
            Self::File { path, .. } => match fs::symlink_metadata(path) {
                Ok(metadata) if metadata.is_file() && metadata.len() > 0 => None,
                _ => Some(UNAVAILABLE_BUILD),
            },
        }
    }

    /// Reads the file and checks it against the manifest's checksum when there is one.
    pub fn load(&self) -> Result<Payload, String> {
        let Self::File {
            path,
            expected_sha256,
        } = self
        else {
            return Err(self
                .unavailable_reason()
                .unwrap_or(UNAVAILABLE_BUILD)
                .to_owned());
        };
        let bytes = read_small(path, MAX_PROGRAM_BYTES)
            .map_err(|error| format!("Silo's program could not be read ({error})."))?;
        if bytes.is_empty() {
            return Err("Silo's program is empty.".into());
        }
        let sha256 = sha256_hex(&bytes);
        if let Some(expected) = expected_sha256 {
            if &sha256 != expected {
                return Err(
                    "Silo's program for computers does not match the checksum this build recorded for it, so it was not sent."
                        .into(),
                );
            }
        }
        Ok(Payload { sha256, bytes })
    }
}

/// A program the owner names for development (`silo tools enable --binary`): read as it is, with
/// the checksum computed from the bytes that are sent.
pub fn override_file(path: &Path) -> Result<Payload, String> {
    let located = Located::File {
        path: path.to_owned(),
        expected_sha256: None,
    };
    located.load()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn stage(resources: &Path, manifest: serde_json::Value, program: Option<&[u8]>) {
        let directory = resources.join(RESOURCE_DIRECTORY);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("manifest.json"), manifest.to_string()).unwrap();
        if let Some(program) = program {
            fs::write(directory.join("silo"), program).unwrap();
        }
    }

    fn good_manifest(program: &[u8], architecture: &str) -> serde_json::Value {
        json!({"schemaVersion":1,"available":true,"architecture":architecture,
               "binarySha256": sha256_hex(program), "size": program.len()})
    }

    #[test]
    fn the_shipped_program_is_found_and_checked_against_its_manifest() {
        let dir = tempfile::tempdir().unwrap();
        stage(
            dir.path(),
            good_manifest(b"ELF program", "arm64"),
            Some(b"ELF program"),
        );
        let located = linux(dir.path(), "arm64");
        assert_eq!(located.unavailable_reason(), None);
        let payload = located.load().unwrap();
        assert_eq!(payload.bytes, b"ELF program");
        assert_eq!(payload.sha256, sha256_hex(b"ELF program"));
    }

    #[test]
    fn a_file_that_is_not_the_manifests_is_not_sent() {
        let dir = tempfile::tempdir().unwrap();
        stage(
            dir.path(),
            good_manifest(b"ELF program", "arm64"),
            Some(b"another program"),
        );
        let error = linux(dir.path(), "arm64").load().unwrap_err();
        assert!(error.contains("does not match the checksum"), "{error}");
    }

    #[test]
    fn a_build_without_zig_says_why_and_a_build_with_nothing_staged_says_so() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            matches!(linux(dir.path(), "arm64"), Located::Unavailable(reason) if reason.contains("no Linux program"))
        );
        stage(
            dir.path(),
            json!({"schemaVersion":1,"available":false,"architecture":"arm64","reason":"Zig is not installed. Silo's tools cannot be installed in Linux computers by this build."}),
            None,
        );
        match linux(dir.path(), "arm64") {
            Located::Unavailable(reason) => assert!(reason.starts_with("Zig is not installed")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_program_for_another_architecture_or_a_damaged_manifest_is_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        stage(dir.path(), good_manifest(b"x", "amd64"), Some(b"x"));
        assert!(
            matches!(linux(dir.path(), "arm64"), Located::Unavailable(reason) if reason.contains("amd64"))
        );
        for manifest in [
            json!({"schemaVersion":2,"available":true,"architecture":"arm64","binarySha256":sha256_hex(b"x"),"size":1}),
            json!({"schemaVersion":1,"available":true,"architecture":"arm64","binarySha256":"zz","size":1}),
            json!({"schemaVersion":1,"available":true,"architecture":"arm64","binarySha256":sha256_hex(b"x"),"size":0}),
            json!({"schemaVersion":1,"available":true,"architecture":"arm64","binarySha256":sha256_hex(b"x"),"size":999999999999u64}),
        ] {
            stage(dir.path(), manifest, Some(b"x"));
            assert!(matches!(
                linux(dir.path(), "arm64"),
                Located::Unavailable(_)
            ));
        }
        fs::write(
            dir.path().join(RESOURCE_DIRECTORY).join("manifest.json"),
            "{ not json",
        )
        .unwrap();
        assert!(matches!(
            linux(dir.path(), "arm64"),
            Located::Unavailable(_)
        ));
    }

    #[test]
    fn a_missing_empty_linked_or_oversized_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        stage(dir.path(), good_manifest(b"x", "arm64"), None);
        let located = linux(dir.path(), "arm64");
        assert!(located.unavailable_reason().is_some());
        assert!(located.load().is_err());
        let program = dir.path().join(RESOURCE_DIRECTORY).join("silo");
        let outside = dir.path().join("outside");
        fs::write(&outside, b"x").unwrap();
        std::os::unix::fs::symlink(&outside, &program).unwrap();
        assert!(located.load().is_err(), "a link is not followed");
        fs::remove_file(&program).unwrap();
        fs::write(&program, b"").unwrap();
        assert!(located.load().is_err());
    }

    #[test]
    fn the_device_program_and_an_override_are_hashed_from_the_bytes_that_are_sent() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("silo"), b"device silo").unwrap();
        let payload = device(Some(dir.path())).load().unwrap();
        assert_eq!(payload.sha256, sha256_hex(b"device silo"));
        assert!(matches!(device(None), Located::Unavailable(_)));
        assert_eq!(
            override_file(&dir.path().join("silo")).unwrap().bytes,
            b"device silo"
        );
        assert!(override_file(&dir.path().join("missing")).is_err());
        assert!(override_file(dir.path()).is_err());
    }
}
