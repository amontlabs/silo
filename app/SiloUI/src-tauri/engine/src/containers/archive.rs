//! The payload of a container computer in a `.silo-backup` file: `snapshot save` writes it from a
//! checkpoint and `snapshot load` makes a checkpoint of it, in the shapes the export and import
//! already use for a virtual machine's MicroSandbox snapshot.
//!
//! A checkpoint is a chain of layers (see [`super::layers`]), so the payload is the chain: a
//! header, then each layer as its own zstd frame of a standard tar (PAX) stream that
//! `silo-ctr pack` writes and `silo-ctr unpack` reads (sockets are left out: tar has no type for
//! one). Nothing is merged or
//! copied first, so an export needs no disk space beyond the file. The guest image does not travel:
//! the header names the image the layers sit on, and the importing Silo needs the same one.
//!
//! <pre>
//! "SILO-CTR-EXPORT1" | header length (u32) | header (JSON) | per layer: frame length (u64) | zstd frame
//! </pre>
//!
//! An import reads a file somebody else made. Each layer is unpacked by `silo-ctr unpack` in the
//! user namespace of a container, with the new layer as its root and a limit on memory and
//! the disk's real free bytes and inodes watched (see the shim's `pack` module). The header is
//! checked field by field before any layer is made, and nothing in it can name a secret, a host
//! path or another checkpoint.

use super::args::{SnapshotLoad, SnapshotSave};
use super::layout::{ComputerRecord, RECORD_VERSION};
use crate::runtime::RuntimeError;
use crate::runtime::RuntimePaths;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Read;
use std::path::Path;

pub const MAGIC: &[u8; 16] = b"SILO-CTR-EXPORT1";
const MAX_HEADER_BYTES: u32 = 1 << 20;
/// More layers than a chain can have (see `layers::MAX_CHAIN`), with room for a longer one.
pub const MAX_LAYERS: u32 = 64;
/// The largest value of a size or count in a header; nothing a person can make is larger.
const MAX_RESOURCE: u64 = 1 << 40;

/// What the file says about the computer: the checkpoint's name and the record it was made from,
/// without the secrets, GitHub access, environment and layer ids the importing Silo assigns.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Header {
    pub name: String,
    pub template: ComputerRecord,
    pub layers: u32,
}

/// The template an export carries: the record without what the importing Silo decides.
pub fn portable_template(record: &ComputerRecord, name: &str) -> ComputerRecord {
    let mut template = record.clone();
    template.name = name.to_owned();
    template.layers = Vec::new();
    template.upper = String::new();
    template.env = Vec::new();
    template.secrets = Vec::new();
    template.github = false;
    template.network_blocked = false;
    template.started_once = false;
    template
}

fn invalid(detail: impl Into<String>) -> RuntimeError {
    RuntimeError::Invalid(detail.into())
}

fn corrupt(detail: &str) -> RuntimeError {
    invalid(format!(
        "The export file's container data is not usable: {detail}."
    ))
}

fn is_sha256(text: &str) -> bool {
    text.strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

impl Header {
    /// Whether the header is one this Silo may build a computer from.
    pub fn validate(&self) -> Result<(), String> {
        let template = &self.template;
        let bad = |detail: &str| Err(format!("its settings are not usable ({detail})"));
        if !super::layers::valid_name(&self.name) {
            return bad("the checkpoint name");
        }
        if self.layers == 0 || self.layers > MAX_LAYERS {
            return bad("the number of layers");
        }
        if template.schema_version != RECORD_VERSION {
            return bad("the record version");
        }
        if crate::runtime::validate_name(&template.name).is_err() {
            return bad("the computer name");
        }
        if !is_sha256(&template.image_digest)
            || template.image_reference.is_empty()
            || template.image_reference.len() > 512
            || template.image_reference.contains(['\0', '\n'])
        {
            return bad("the guest image");
        }
        let sizes = [
            template.cpus,
            template.max_cpus,
            template.memory_mib,
            template.max_memory_mib,
            template.root_disk_mib,
            template.workspace_mib,
        ];
        if sizes.iter().any(|size| *size == 0 || *size > MAX_RESOURCE)
            || template.cpus > template.max_cpus
            || template.memory_mib > template.max_memory_mib
        {
            return bad("the resources");
        }
        if template.labels.len() > 64
            || template.labels.iter().any(|(key, value)| {
                key.is_empty()
                    || key.contains(['=', '\0'])
                    || value.contains('\0')
                    || value.len() > 1024
            })
        {
            return bad("the labels");
        }
        if !template.layers.is_empty()
            || !template.upper.is_empty()
            || !template.env.is_empty()
            || !template.secrets.is_empty()
            || template.github
            || template.started_once
            || template.network_blocked
        {
            return bad("it carries something an export does not");
        }
        Ok(())
    }

    /// Whether the header describes the computer the manifest does (`runtime_config` is the
    /// `inspect` answer the export recorded): the file must not turn a small computer into a
    /// large one, or one on another image, past what the manifest was checked for.
    pub fn matches(&self, runtime_config: &Value) -> Result<(), String> {
        let template = &self.template;
        let number = |pointer: &str| runtime_config.pointer(pointer).and_then(Value::as_u64);
        // The sizes the host disk guard reads come from the header, so each must be there and
        // equal (a missing one is a manifest that does not describe this computer).
        let same = number("/resources/cpus") == Some(template.cpus)
            && number("/resources/max_cpus") == Some(template.max_cpus)
            && number("/resources/memory_mib") == Some(template.memory_mib)
            && number("/resources/max_memory_mib") == Some(template.max_memory_mib)
            && runtime_config
                .pointer("/image/Oci/reference")
                .and_then(Value::as_str)
                == Some(template.image_reference.as_str())
            && number("/image/Oci/root_disk/size_mib") == Some(template.root_disk_mib)
            && number("/mounts/0/storage/capacity_mib") == Some(template.workspace_mib);
        if same {
            Ok(())
        } else {
            Err("its container data does not match its settings".into())
        }
    }
}

/// Reads and checks the header of a payload file.
pub fn read_header(path: &Path) -> Result<Header, RuntimeError> {
    let mut file = std::fs::File::open(path).map_err(|error| {
        invalid(format!(
            "The export file's container data could not be read: {error}."
        ))
    })?;
    read_header_from(&mut file)
}

fn read_header_from(file: &mut impl Read) -> Result<Header, RuntimeError> {
    let mut magic = [0u8; 16];
    file.read_exact(&mut magic)
        .map_err(|_| corrupt("it is too short"))?;
    if &magic != MAGIC {
        return Err(corrupt("it is not container data"));
    }
    let mut length = [0u8; 4];
    file.read_exact(&mut length)
        .map_err(|_| corrupt("it is too short"))?;
    let length = u32::from_be_bytes(length);
    if length == 0 || length > MAX_HEADER_BYTES {
        return Err(corrupt("its header has a bad size"));
    }
    let mut bytes = vec![0u8; length as usize];
    file.read_exact(&mut bytes)
        .map_err(|_| corrupt("its header is incomplete"))?;
    let header: Header =
        serde_json::from_slice(&bytes).map_err(|_| corrupt("its header is malformed"))?;
    header.validate().map_err(|detail| {
        invalid(format!(
            "The export file's container data is not usable: {detail}."
        ))
    })?;
    Ok(header)
}

/// The refusal for a device that cannot run a container computer, from the host check.
pub fn require_container_host() -> Result<(), RuntimeError> {
    super::require_host().map_err(|error| {
        invalid(format!(
            "Silo cannot import a container computer on this device. {error}"
        ))
    })
}

/// Whether this device has the guest image with config digest `digest`: the one this Silo pins
/// (unpacked when a computer needs it) or one kept unpacked for a computer made on it. The
/// layers hold changes against exactly that image, and the file does not carry it.
fn has_image(home: &Path, digest: &str) -> bool {
    is_sha256(digest)
        && (super::image::pinned().is_ok_and(|pinned| pinned.image_digest == digest)
            || super::image::unpacked_digest(&super::layout::Layout::new(home), digest, "")
                .is_some())
}

/// An export names the image its computer was made on, whichever build pinned it then: it only
/// needs to be one this device has.
pub fn require_exportable_image(home: &Path, header: &Header) -> Result<(), RuntimeError> {
    if has_image(home, &header.template.image_digest) {
        return Ok(());
    }
    Err(invalid(
        "This computer was made on a guest image that is no longer on this device, so it cannot be exported.",
    ))
}

/// An import needs the image the file's layers sit on.
pub fn require_importable_image(home: &Path, digest: &str) -> Result<(), RuntimeError> {
    if has_image(home, digest) {
        return Ok(());
    }
    Err(invalid(
        "This computer was made on an older guest image that this device does not have.",
    ))
}

#[cfg(target_os = "linux")]
pub fn save(paths: &RuntimePaths, save: &SnapshotSave) -> Result<(), RuntimeError> {
    linux::save(paths, save)
}

#[cfg(target_os = "linux")]
pub fn load(paths: &RuntimePaths, load: &SnapshotLoad) -> Result<(), RuntimeError> {
    linux::load(paths, load)
}

#[cfg(not(target_os = "linux"))]
pub fn save(_: &RuntimePaths, _: &SnapshotSave) -> Result<(), RuntimeError> {
    Err(RuntimeError::Unavailable(
        "Container computers run only on a Linux host.".into(),
    ))
}

#[cfg(not(target_os = "linux"))]
pub fn load(_: &RuntimePaths, _: &SnapshotLoad) -> Result<(), RuntimeError> {
    Err(RuntimeError::Unavailable(
        "Container computers run only on a Linux host.".into(),
    ))
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use crate::containers::{
        layers::{self, Snapshot, Store},
        layout::Layout,
        lifecycle,
        tools::Tools,
    };
    use std::fs::File;
    use std::io::{BufWriter, Seek, SeekFrom, Write};
    use std::process::{Command, Stdio};

    fn unavailable(detail: impl Into<String>) -> RuntimeError {
        RuntimeError::Unavailable(detail.into())
    }

    /// `silo-ctr userns --keep-id N -- silo-ctr <verb> ...`: the shim's helper as the container's
    /// owner, where the layers' file owners are real.
    fn helper(tools: &Tools, verb: &str, arguments: &[&std::ffi::OsStr]) -> Command {
        let mut command = Command::new(&tools.ctr);
        command
            .args(["userns", "--keep-id"])
            .arg(crate::containers::idmap::AGENT_ID.to_string())
            .arg("--")
            .arg(&tools.ctr)
            .arg(verb)
            .args(arguments);
        command
    }

    /// What the helper printed on standard error, read to the end on a thread so a full pipe
    /// never stops it; the last line says why it failed.
    fn collect_errors(
        stderr: Option<std::process::ChildStderr>,
    ) -> std::thread::JoinHandle<String> {
        std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(mut stderr) = stderr {
                let mut bytes = Vec::new();
                let _ = (&mut stderr).take(1 << 16).read_to_end(&mut bytes);
                // Keep draining so the helper is never blocked on a full pipe.
                let _ = std::io::copy(&mut stderr, &mut std::io::sink());
                text = String::from_utf8_lossy(&bytes).into_owned();
            }
            text
        })
    }

    fn last_line(text: &str) -> String {
        text.lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("it printed no reason")
            .to_owned()
    }

    pub fn save(paths: &RuntimePaths, request: &SnapshotSave) -> Result<(), RuntimeError> {
        super::super::require_host()?;
        let tools = Tools::locate(paths)?;
        let layout = Layout::new(&paths.home);
        let snapshot = Store::new(&layout)
            .read(&request.group, &request.name)
            .map_err(invalid)?;
        if !layers::intact(&layout, &snapshot) {
            return Err(invalid(format!(
                "The checkpoint {}:{} is missing some of its files, so it cannot be exported.",
                request.group, request.name
            )));
        }
        let header = Header {
            name: snapshot.name.clone(),
            template: portable_template(&snapshot.template, &snapshot.template.name),
            layers: u32::try_from(snapshot.layers.len()).unwrap_or(u32::MAX),
        };
        header
            .validate()
            .map_err(|detail| invalid(format!("This container cannot be exported: {detail}.")))?;
        write_payload(&tools, &layout, &snapshot, &header, &request.file)
    }

    /// Writes the payload to the new file `out`. A file that is already there is refused and
    /// left as it is; a file this call made is removed when the payload could not be finished.
    fn write_payload(
        tools: &Tools,
        layout: &Layout,
        snapshot: &Snapshot,
        header: &Header,
        out: &Path,
    ) -> Result<(), RuntimeError> {
        use std::os::unix::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(out)
            .map_err(|error| unavailable(format!("The export file could not be made: {error}.")))?;
        let result = write_layers(tools, layout, snapshot, header, file);
        if result.is_err() {
            let _ = std::fs::remove_file(out);
        }
        result
    }

    fn write_layers(
        tools: &Tools,
        layout: &Layout,
        snapshot: &Snapshot,
        header: &Header,
        mut file: File,
    ) -> Result<(), RuntimeError> {
        let io = |error: std::io::Error| {
            unavailable(format!("The export file could not be written: {error}."))
        };
        let header_bytes = serde_json::to_vec(header)
            .map_err(|_| unavailable("The export header could not be made."))?;
        file.write_all(MAGIC).map_err(io)?;
        file.write_all(&(header_bytes.len() as u32).to_be_bytes())
            .map_err(io)?;
        file.write_all(&header_bytes).map_err(io)?;
        for id in &snapshot.layers {
            let length_at = file.stream_position().map_err(io)?;
            file.write_all(&0u64.to_be_bytes()).map_err(io)?;
            let layer = layout.layer(id);
            let mut child = helper(tools, "pack", &[layer.as_os_str()])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|error| {
                    unavailable(format!("{} could not start: {error}", tools.ctr.display()))
                })?;
            let errors = collect_errors(child.stderr.take());
            let mut stdout = child.stdout.take().expect("piped stdout");
            let start = file.stream_position().map_err(io)?;
            let copied = {
                let mut encoder =
                    zstd::Encoder::new(BufWriter::with_capacity(1 << 20, &mut file), 3)
                        .map_err(io)?;
                let copied = std::io::copy(&mut stdout, &mut encoder);
                let finished = encoder.finish().and_then(|mut writer| writer.flush());
                copied.and(finished)
            };
            drop(stdout);
            let status = child.wait().map_err(io)?;
            let reason = errors.join().unwrap_or_default();
            packed(copied, status.success(), &reason)?;
            let end = file.stream_position().map_err(io)?;
            file.seek(SeekFrom::Start(length_at)).map_err(io)?;
            file.write_all(&(end - start).to_be_bytes()).map_err(io)?;
            file.seek(SeekFrom::Start(end)).map_err(io)?;
        }
        file.sync_all().map_err(io)
    }

    /// Whether a layer reached the export file, from how the copy into the file and the helper
    /// ended. A file that could not be written (a full disk) also ends the helper, with a broken
    /// pipe, so the write is the reason to give when both failed.
    fn packed(
        copied: std::io::Result<()>,
        helper_succeeded: bool,
        helper_errors: &str,
    ) -> Result<(), RuntimeError> {
        copied.map_err(|error| {
            unavailable(format!("The export file could not be written: {error}."))
        })?;
        if helper_succeeded {
            Ok(())
        } else {
            Err(unavailable(format!(
                "Packing the computer's files failed: {}.",
                last_line(helper_errors)
            )))
        }
    }

    pub fn load(paths: &RuntimePaths, request: &SnapshotLoad) -> Result<(), RuntimeError> {
        require_container_host()?;
        let tools = Tools::locate(paths)?;
        let layout = Layout::new(&paths.home);
        let mut file = File::open(&request.file).map_err(|error| {
            invalid(format!(
                "The export file's container data could not be read: {error}."
            ))
        })?;
        let header = read_header_from(&mut file)?;
        require_importable_image(&paths.home, &header.template.image_digest)?;
        crate::containers::host_disk::require_room(&paths.home, "import a container computer")?;
        let store = Store::new(&layout);
        if store.group_exists(&request.group) {
            return Err(invalid(format!(
                "The import group {} already exists.",
                request.group
            )));
        }
        let ids: Vec<String> = (0..header.layers).map(|_| layers::new_id()).collect();
        // Whatever fails from here on removes what was made (layers and record), so the guard
        // exists before the first layer does.
        let created = Created {
            layout: &layout,
            tools: &tools,
            group: &request.group,
            name: &header.name,
            ids: &ids,
            keep: std::cell::Cell::new(false),
        };
        // The record that names the layers is written first, so collection never takes a layer
        // that is still being filled; it says so (`filling`), so a layer half unpacked is not a
        // checkpoint whatever stops the import.
        let mut snapshot = {
            let _store = layers::lock();
            for id in &ids {
                lifecycle::make_layer(&tools, &layout.layer(id)).map_err(unavailable)?;
            }
            let mut template = header.template.clone();
            template.layers = ids.clone();
            let mut snapshot = Snapshot::new(&request.group, &header.name, ids.clone(), template);
            snapshot.filling = true;
            store.write(&snapshot).map_err(unavailable)?;
            snapshot
        };
        for id in &ids {
            let mut length = [0u8; 8];
            file.read_exact(&mut length)
                .map_err(|_| corrupt("a layer is missing"))?;
            let length = u64::from_be_bytes(length);
            unpack_layer(&tools, &layout.layer(id), &mut file, length)?;
        }
        let mut extra = [0u8; 1];
        if file.read(&mut extra).unwrap_or(0) != 0 {
            return Err(corrupt("it has data after its last layer"));
        }
        snapshot.filling = false;
        store.write(&snapshot).map_err(unavailable)?;
        created.keep.set(true);
        Ok(())
    }

    /// The layers and checkpoint record of a load, removed unless it finished.
    struct Created<'a> {
        layout: &'a Layout,
        tools: &'a Tools,
        group: &'a str,
        name: &'a str,
        ids: &'a [String],
        keep: std::cell::Cell<bool>,
    }

    impl Drop for Created<'_> {
        fn drop(&mut self) {
            if self.keep.get() {
                return;
            }
            let _store = layers::lock();
            let _ = Store::new(self.layout).remove(self.group, self.name);
            for id in self.ids {
                let _ = crate::containers::image::remove_tree(self.tools, &self.layout.layer(id));
            }
        }
    }

    /// Feeds one frame of the file to `silo-ctr unpack`, which keeps the margin of the host disk
    /// guard free in bytes and in inodes.
    fn unpack_layer(
        tools: &Tools,
        layer: &Path,
        file: &mut File,
        length: u64,
    ) -> Result<(), RuntimeError> {
        let min_bytes = crate::containers::host_disk::margin_bytes().to_string();
        let min_inodes = crate::containers::host_disk::margin_inodes().to_string();
        let mut child = helper(
            tools,
            "unpack",
            &[
                "--min-free-bytes".as_ref(),
                min_bytes.as_ref(),
                "--min-free-inodes".as_ref(),
                min_inodes.as_ref(),
                layer.as_os_str(),
            ],
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            unavailable(format!("{} could not start: {error}", tools.ctr.display()))
        })?;
        let errors = collect_errors(child.stderr.take());
        let mut stdin = child.stdin.take().expect("piped stdin");
        let fed = (|| -> std::io::Result<()> {
            let mut decoder =
                zstd::Decoder::new(std::io::BufReader::new(Read::by_ref(file).take(length)))?;
            std::io::copy(&mut decoder, &mut stdin)?;
            Ok(())
        })();
        drop(stdin);
        let status = child
            .wait()
            .map_err(|error| unavailable(error.to_string()))?;
        let reason = errors.join().unwrap_or_default();
        if !status.success() {
            return Err(invalid(format!(
                "Unpacking the computer's files failed: {}.",
                last_line(&reason)
            )));
        }
        fed.map_err(|_| corrupt("a layer is damaged"))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_failed_export_removes_the_file_it_made_and_no_other() {
            let home = tempfile::tempdir().unwrap();
            let layout = Layout::new(home.path());
            crate::containers::capability::make_container(home.path(), "box");
            let record = layout.computer("box").read_record().unwrap();
            let snapshot = Snapshot::new("box", "c1", vec!["l1".into()], record.clone());
            let header = Header {
                name: "c1".into(),
                template: portable_template(&record, "box"),
                layers: 1,
            };
            // No helper to run: the export fails after the file was made.
            let missing = home.path().join("no-such-helper");
            let tools = Tools {
                ctr: missing.clone(),
                init: missing.clone(),
                gateway: missing.clone(),
                crun: missing.clone(),
                skopeo: missing.clone(),
                umoci: missing.clone(),
                systemd_run: missing.clone(),
                systemctl: missing,
            };
            let out = home.path().join("payload");
            assert!(write_payload(&tools, &layout, &snapshot, &header, &out).is_err());
            assert!(!out.exists());
            // A file that was already there is somebody else's: refused, and still there.
            std::fs::write(&out, "kept").unwrap();
            let error = write_payload(&tools, &layout, &snapshot, &header, &out).unwrap_err();
            assert!(error.to_string().contains("could not be made"), "{error}");
            assert_eq!(std::fs::read_to_string(&out).unwrap(), "kept");
        }

        #[test]
        fn a_full_disk_is_named_instead_of_the_helpers_broken_pipe() {
            let full = || std::io::Error::from_raw_os_error(libc::ENOSPC);
            let error = packed(Err(full()), false, "silo-ctr: write the stream: Broken pipe\n")
                .unwrap_err()
                .to_string();
            assert!(
                error.starts_with("The export file could not be written:")
                    && !error.contains("Broken pipe"),
                "{error}"
            );
            // The helper's own failure (a device in the layer) is still the reason when the
            // file took everything the helper wrote.
            let error = packed(Ok(()), false, "silo-ctr: /dev/x is a device\n")
                .unwrap_err()
                .to_string();
            assert!(error.contains("is a device"), "{error}");
            assert!(packed(Ok(()), true, "").is_ok());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::containers::{capability::make_container, layout::Layout};

    fn template() -> ComputerRecord {
        let home = tempfile::tempdir().unwrap();
        make_container(home.path(), "box");
        let mut record = Layout::new(home.path())
            .computer("box")
            .read_record()
            .unwrap();
        record.image_digest = format!("sha256:{}", "a".repeat(64));
        portable_template(&record, "box")
    }

    fn header() -> Header {
        Header {
            name: "silo-backup-0-1-2".into(),
            template: template(),
            layers: 2,
        }
    }

    fn bytes(header: &Header) -> Vec<u8> {
        let json = serde_json::to_vec(header).unwrap();
        let mut file = MAGIC.to_vec();
        file.extend_from_slice(&(json.len() as u32).to_be_bytes());
        file.extend_from_slice(&json);
        file
    }

    #[test]
    fn a_header_round_trips_and_validates() {
        let file = bytes(&header());
        assert_eq!(read_header_from(&mut file.as_slice()).unwrap(), header());
    }

    #[test]
    fn the_portable_template_drops_what_the_importing_silo_decides() {
        let mut record = template();
        record.layers = vec!["l1".into()];
        record.upper = "l2".into();
        record.env = vec![("A".into(), "b".into())];
        record.secrets = vec![crate::containers::layout::SecretRecord {
            name: "TOKEN".into(),
            allowed_hosts: vec!["example.com".into()],
        }];
        record.github = true;
        record.started_once = true;
        record.network_blocked = true;
        let portable = portable_template(&record, "other");
        assert_eq!(portable.name, "other");
        assert!(portable.layers.is_empty() && portable.upper.is_empty() && portable.env.is_empty());
        assert!(
            portable.secrets.is_empty()
                && !portable.github
                && !portable.started_once
                && !portable.network_blocked
        );
        assert_eq!(portable.cpus, record.cpus);
        assert_eq!(portable.image_digest, record.image_digest);
    }

    #[test]
    fn a_header_that_could_smuggle_something_is_refused() {
        let mut cases: Vec<(&str, Header)> = Vec::new();
        let mut one = header();
        one.name = "../x".into();
        cases.push(("name", one));
        let mut one = header();
        one.layers = 0;
        cases.push(("zero layers", one));
        let mut one = header();
        one.layers = MAX_LAYERS + 1;
        cases.push(("many layers", one));
        let mut one = header();
        one.template.layers = vec!["l1".into()];
        cases.push(("layer ids", one));
        let mut one = header();
        one.template.secrets = vec![crate::containers::layout::SecretRecord {
            name: "TOKEN".into(),
            allowed_hosts: Vec::new(),
        }];
        cases.push(("secrets", one));
        let mut one = header();
        one.template.github = true;
        cases.push(("github", one));
        let mut one = header();
        one.template.network_blocked = true;
        cases.push(("network", one));
        let mut one = header();
        one.template.image_digest = "latest".into();
        cases.push(("image", one));
        let mut one = header();
        one.template.memory_mib = MAX_RESOURCE + 1;
        one.template.max_memory_mib = MAX_RESOURCE + 1;
        cases.push(("memory", one));
        let mut one = header();
        one.template.cpus = 0;
        cases.push(("cpus", one));
        let mut one = header();
        one.template.labels = vec![("a=b".into(), "c".into())];
        cases.push(("label", one));
        let mut one = header();
        one.template.name = "bad name".into();
        cases.push(("computer name", one));
        for (what, case) in cases {
            assert!(case.validate().is_err(), "{what}");
        }
    }

    #[test]
    fn a_damaged_file_is_named_not_guessed_at() {
        let good = bytes(&header());
        for (cut, what) in [
            (0, "empty"),
            (10, "magic"),
            (18, "length"),
            (good.len() - 1, "header"),
        ] {
            let error = read_header_from(&mut &good[..cut]).unwrap_err().to_string();
            assert!(error.contains("not usable"), "{what}: {error}");
        }
        let mut wrong = good.clone();
        wrong[0] = b'X';
        assert!(read_header_from(&mut wrong.as_slice())
            .unwrap_err()
            .to_string()
            .contains("not container data"));
        let mut big = MAGIC.to_vec();
        big.extend_from_slice(&u32::MAX.to_be_bytes());
        assert!(read_header_from(&mut big.as_slice()).is_err());
        let mut extra = serde_json::to_value(header()).unwrap();
        extra["surprise"] = Value::Bool(true);
        let json = serde_json::to_vec(&extra).unwrap();
        let mut file = MAGIC.to_vec();
        file.extend_from_slice(&(json.len() as u32).to_be_bytes());
        file.extend_from_slice(&json);
        assert!(read_header_from(&mut file.as_slice())
            .unwrap_err()
            .to_string()
            .contains("malformed"));
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn a_device_that_cannot_run_containers_refuses_an_import_in_plain_words() {
        let message = require_container_host().unwrap_err().to_string();
        assert!(
            message.starts_with("Silo cannot import a container computer on this device.")
                && message.contains("Linux"),
            "{message}"
        );
    }

    #[test]
    fn the_header_must_describe_the_computer_the_manifest_does() {
        let home = tempfile::tempdir().unwrap();
        make_container(home.path(), "box");
        let record = Layout::new(home.path())
            .computer("box")
            .read_record()
            .unwrap();
        let config =
            crate::containers::inspect_value("box", &record, "Stopped", &[])["config"].clone();
        let mut same = header();
        same.template = portable_template(&record, "box");
        same.template.image_digest = format!("sha256:{}", "a".repeat(64));
        assert!(same.matches(&config).is_ok());
        let mut larger = same.clone();
        larger.template.memory_mib += 1;
        larger.template.max_memory_mib += 1;
        assert!(larger.matches(&config).is_err());
        let mut elsewhere = same.clone();
        elsewhere.template.image_reference = "other:1".into();
        assert!(elsewhere.matches(&config).is_err());
        let mut disk = same.clone();
        disk.template.root_disk_mib += 1;
        assert!(disk.matches(&config).is_err());
        let mut workspace = same.clone();
        workspace.template.workspace_mib += 1;
        assert!(workspace.matches(&config).is_err());
        // A manifest that leaves a size out does not describe this computer.
        for pointer in ["/image/Oci/root_disk", "/mounts"] {
            let mut missing = config.clone();
            let (parent, key) = pointer.rsplit_once('/').unwrap();
            let parent = if parent.is_empty() {
                &mut missing
            } else {
                missing.pointer_mut(parent).unwrap()
            };
            parent.as_object_mut().unwrap().remove(key);
            assert!(same.matches(&missing).is_err(), "{pointer}");
        }
    }
}
