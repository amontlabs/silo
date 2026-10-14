//! The format-4 Apple Virtualization payload codec.
//!
//! The outer `.silo-backup` envelope stays owned by `backup.rs`. This module
//! encodes and verifies the ordered component stream within each computer row.

use super::{
    BackupError, BackupResult, BackupService, Cancellation, MsbRunner, PackageComputer,
    PackageManifest, RuntimeManifest, FORMAT_VERSION,
    FREE_SPACE_RESERVE, MAX_MANIFEST_BYTES, MAGIC, DEFAULT_MAX_ARCHIVE_BYTES,
    check_cancelled, format_bytes, now_ms, open_regular_file, pending_digest,
    read_and_verify_package, validate_manifest, write_immutable_package,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub(super) const RUNTIME_NAME: &str = "apple-virtualization";
const RUNTIME_VERSION: &str = "1";
const SNAPSHOT_FORMAT: &str = "silo-vz-components-v1";
const STREAM_MAGIC: &[u8; 16] = b"SILO-VZ-COMP\0\0\0\0";
const STREAM_VERSION: u32 = 1;
const STREAM_FIXED_HEADER_BYTES: u64 = 24;
const STREAM_INDEX_ENTRY_BYTES: u64 = 52;
const MAX_WINDOW_LOG: u32 = 27; // 128 MiB
const MAX_LOGICAL_COMPONENT_BYTES: u64 = 8 * 1024 * 1024 * 1024 * 1024;
const GIB: u64 = 1024 * 1024 * 1024;
const MAX_CPUS: u64 = 256;
const MAX_MEMORY_GIB: u64 = 1024;
const MIN_MEMORY_GIB: u64 = 4;
const MIN_CPUS: u64 = 2;
const MIN_DISK_GIB: u64 = 32;
const MAX_DISK_GIB: u64 = 1024;
const MAX_TEXT_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum MacosComponentRole {
    Disk,
    Auxiliary,
    HardwareModel,
    MachineIdentifier,
    Memory,
}

impl MacosComponentRole {
    fn index(self) -> u32 {
        match self {
            Self::Disk => 1,
            Self::Auxiliary => 2,
            Self::HardwareModel => 3,
            Self::MachineIdentifier => 4,
            Self::Memory => 5,
        }
    }

    fn file_name(self) -> &'static str {
        match self {
            Self::Disk => "disk.img",
            Self::Auxiliary => "auxiliary-storage.img",
            Self::HardwareModel => "hardware-model.bin",
            Self::MachineIdentifier => "machine-identifier.bin",
            Self::Memory => "state.vzvmsave",
        }
    }

    fn json_name(self) -> &'static str {
        match self {
            Self::Disk => "disk",
            Self::Auxiliary => "auxiliary",
            Self::HardwareModel => "hardwareModel",
            Self::MachineIdentifier => "machineIdentifier",
            Self::Memory => "memory",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum MacosCheckpointKind {
    Disk,
    Memory,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum MacosArchiveSourceKind {
    WholeComputer,
    Checkpoint {
        id: String,
        name: String,
        kind: MacosCheckpointKind,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum MacosArchiveMemory {
    None,
    Saved {
        host_build: String,
        configuration_digest: String,
        size: u64,
        sha256: String,
    },
}

/// Typed source metadata accepted by the writer. The writer supplies the
/// ordered component index after it has compressed and hashed the input files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MacosArchiveConfig {
    pub(crate) payload_version: u32,
    pub(crate) architecture: String,
    pub(crate) source_computer_id: String,
    pub(crate) source_name: String,
    pub(crate) cpus: u64,
    pub(crate) memory_gib: u64,
    pub(crate) disk_gib: u64,
    pub(crate) guest_version: String,
    pub(crate) guest_build: String,
    pub(crate) host_build: String,
    pub(crate) hardware_model_digest: String,
    pub(crate) setup_fingerprint: String,
    pub(crate) computer_use_fingerprint: String,
    pub(crate) source: MacosArchiveSourceKind,
    pub(crate) memory: MacosArchiveMemory,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MacosArchiveComponent {
    pub(crate) role: MacosComponentRole,
    pub(crate) logical_length: u64,
    pub(crate) stored_length: u64,
    pub(crate) sha256: String,
}

/// The strict runtime configuration stored in `PackageComputer.runtimeConfig`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MacosArchiveConfiguration {
    pub(crate) payload_version: u32,
    pub(crate) architecture: String,
    pub(crate) source_computer_id: String,
    pub(crate) source_name: String,
    pub(crate) cpus: u64,
    pub(crate) memory_gib: u64,
    pub(crate) disk_gib: u64,
    pub(crate) guest_version: String,
    pub(crate) guest_build: String,
    pub(crate) host_build: String,
    pub(crate) hardware_model_digest: String,
    pub(crate) setup_fingerprint: String,
    pub(crate) computer_use_fingerprint: String,
    pub(crate) source: MacosArchiveSourceKind,
    pub(crate) memory: MacosArchiveMemory,
    pub(crate) components: Vec<MacosArchiveComponent>,
}

impl MacosArchiveConfiguration {
    fn from_source(
        source: MacosArchiveConfig,
        components: Vec<MacosArchiveComponent>,
    ) -> Self {
        Self {
            payload_version: source.payload_version,
            architecture: source.architecture,
            source_computer_id: source.source_computer_id,
            source_name: source.source_name,
            cpus: source.cpus,
            memory_gib: source.memory_gib,
            disk_gib: source.disk_gib,
            guest_version: source.guest_version,
            guest_build: source.guest_build,
            host_build: source.host_build,
            hardware_model_digest: source.hardware_model_digest,
            setup_fingerprint: source.setup_fingerprint,
            computer_use_fingerprint: source.computer_use_fingerprint,
            source: source.source,
            memory: source.memory,
            components,
        }
    }

    fn source_config(&self) -> MacosArchiveConfig {
        MacosArchiveConfig {
            payload_version: self.payload_version,
            architecture: self.architecture.clone(),
            source_computer_id: self.source_computer_id.clone(),
            source_name: self.source_name.clone(),
            cpus: self.cpus,
            memory_gib: self.memory_gib,
            disk_gib: self.disk_gib,
            guest_version: self.guest_version.clone(),
            guest_build: self.guest_build.clone(),
            host_build: self.host_build.clone(),
            hardware_model_digest: self.hardware_model_digest.clone(),
            setup_fingerprint: self.setup_fingerprint.clone(),
            computer_use_fingerprint: self.computer_use_fingerprint.clone(),
            source: self.source.clone(),
            memory: self.memory.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct MacosArchiveComponentInput {
    pub(crate) role: MacosComponentRole,
    pub(crate) path: PathBuf,
}

#[derive(Clone, Debug)]
pub(crate) struct MacosArchiveSource {
    pub(crate) name: String,
    pub(crate) runtime_config: MacosArchiveConfig,
    pub(crate) computer_configuration: Value,
    pub(crate) components: Vec<MacosArchiveComponentInput>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ArchiveRuntimeKind {
    MicroSandbox,
    AppleVirtualization,
    Mixed,
}

/// Selected files in a private directory on the destination filesystem.
/// Dropping this value removes the stage.
pub(crate) struct PreparedMacosArchive {
    pub(crate) source_name: String,
    pub(crate) config: MacosArchiveConfiguration,
    pub(crate) computer_configuration: Value,
    pub(crate) component_paths: BTreeMap<MacosComponentRole, PathBuf>,
    _stage: tempfile::TempDir,
}

struct PreparedInput {
    role: MacosComponentRole,
    file: File,
    logical_length: u64,
}

struct PreparedSource {
    name: String,
    runtime_config: MacosArchiveConfig,
    computer_configuration: Value,
    inputs: Vec<PreparedInput>,
}

impl<R: MsbRunner> BackupService<R> {
    /// Create a format-4 Apple Virtualization archive from immutable capture or
    /// checkpoint files. All source files are streamed through independent
    /// zstd frames before the shared atomic package publisher commits the file.
    pub(crate) fn create_macos_archive(
        &self,
        destination: &Path,
        sources: Vec<MacosArchiveSource>,
        cancellation: &Cancellation,
        token: Option<&str>,
    ) -> Result<BackupResult, BackupError> {
        let _operation = self.begin()?;
        check_cancelled(cancellation)?;
        if sources.is_empty() || sources.len() > 64 {
            return Err(BackupError::InvalidRequest(
                "Choose between 1 and 64 computers to export.".into(),
            ));
        }
        fs::create_dir_all(&self.scratch_root)?;

        let mut names = HashSet::new();
        let mut prepared = Vec::with_capacity(sources.len());
        let mut logical_total = 0_u64;
        for source in sources {
            super::validate_computer_name(&source.name)
                .map_err(|error| BackupError::InvalidRequest(error.to_string()))?;
            if !names.insert(source.name.clone()) {
                return Err(BackupError::InvalidRequest(
                    "Computer names must be unique in an export.".into(),
                ));
            }
            validate_config_base(&source.runtime_config, &source.name)
                .map_err(|error| BackupError::InvalidRequest(error.to_string()))?;
            validate_computer_configuration(&source.computer_configuration)
                .map_err(|error| BackupError::InvalidRequest(error.to_string()))?;

            let expected_roles = expected_roles(&source.runtime_config.memory);
            let mut by_role = BTreeMap::new();
            for input in source.components {
                if by_role.insert(input.role, input.path).is_some() {
                    return Err(BackupError::InvalidRequest(
                        "The macOS export contains a duplicate component role.".into(),
                    ));
                }
            }
            if by_role.keys().copied().collect::<Vec<_>>() != expected_roles {
                return Err(BackupError::InvalidRequest(
                    "The macOS export components do not match the selected source.".into(),
                ));
            }
            let mut inputs = Vec::with_capacity(expected_roles.len());
            for role in expected_roles {
                let path = by_role.remove(&role).expect("role set checked above");
                let (file, metadata) = open_regular_file(&path).map_err(|error| match error {
                    super::OpenRegularError::NotRegular => BackupError::InvalidRequest(format!(
                        "The {} export component is not a regular file.",
                        role.json_name()
                    )),
                    super::OpenRegularError::Io(error) => BackupError::Io(error),
                })?;
                if metadata.len() == 0 || metadata.len() > MAX_LOGICAL_COMPONENT_BYTES {
                    return Err(BackupError::InvalidRequest(format!(
                        "The {} export component has an unsupported size.",
                        role.json_name()
                    )));
                }
                logical_total = logical_total
                    .checked_add(metadata.len())
                    .filter(|size| *size <= MAX_LOGICAL_COMPONENT_BYTES)
                    .ok_or_else(|| {
                        BackupError::InvalidRequest(
                            "The selected macOS components exceed the logical size limit.".into(),
                        )
                    })?;
                inputs.push(PreparedInput {
                    role,
                    file,
                    logical_length: metadata.len(),
                });
            }
            prepared.push(PreparedSource {
                name: source.name,
                runtime_config: source.runtime_config,
                computer_configuration: source.computer_configuration,
                inputs,
            });
        }

        let stage = self.staging_directory("backup-macos-")?;
        let required_stage = logical_total
            .checked_add(FREE_SPACE_RESERVE)
            .ok_or_else(|| {
                BackupError::InvalidRequest("The macOS export size overflows.".into())
            })?;
        let stage_free = (self.free_space)(stage.path())?;
        if stage_free < required_stage {
            return Err(BackupError::InsufficientSpace(format!(
                "This export needs up to {} of free space for compressed staging in {}; {} is available.",
                super::format_bytes(required_stage),
                stage.path().display(),
                super::format_bytes(stage_free)
            )));
        }

        let mut manifest = PackageManifest {
            schema_version: FORMAT_VERSION,
            created_at_ms: now_ms(),
            runtime: RuntimeManifest {
                name: RUNTIME_NAME.into(),
                version: RUNTIME_VERSION.into(),
                snapshot_format: SNAPSHOT_FORMAT.into(),
                guest_architecture: "aarch64".into(),
            },
            computers: Vec::with_capacity(prepared.len()),
        };
        let mut payloads = Vec::with_capacity(prepared.len());
        let mut staged_payload_files = Vec::with_capacity(prepared.len());
        let mut payload_total = 0_u64;
        for prepared_source in prepared {
            check_cancelled(cancellation)?;
            let mut payload = tempfile::Builder::new()
                .prefix("macos-payload-")
                .tempfile_in(stage.path())?;
            let config = encode_component_stream(
                payload.as_file_mut(),
                prepared_source.inputs,
                prepared_source.runtime_config,
                &prepared_source.name,
                cancellation,
            )?;
            let runtime_config = serde_json::to_value(&config)?;
            let payload_size = payload.as_file().metadata()?.len();
            if payload_size == 0 || payload_size > self.max_archive_bytes {
                return Err(BackupError::InvalidRequest(
                    "A macOS component payload exceeds the export size safety limit.".into(),
                ));
            }
            payload_total = payload_total
                .checked_add(payload_size)
                .filter(|size| *size <= self.max_archive_bytes)
                .ok_or_else(|| {
                    BackupError::InvalidRequest(
                        "The selected macOS exports exceed the archive size safety limit.".into(),
                    )
                })?;
            manifest.computers.push(PackageComputer {
                name: prepared_source.name,
                runtime_config,
                computer_configuration: prepared_source.computer_configuration,
                payload_size,
                payload_sha256: pending_digest(),
                volumes: Vec::new(),
            });
            payloads.push(payload.reopen()?);
            staged_payload_files.push(payload);
        }

        validate_manifest(&manifest)?;
        let manifest_bytes = serde_json::to_vec(&manifest)?;
        if manifest_bytes.is_empty() || manifest_bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(BackupError::InvalidRequest(
                "Export metadata exceeds the supported size.".into(),
            ));
        }
        let archive_size = (MAGIC.len() as u64)
            .checked_add(4 + 8)
            .and_then(|size| size.checked_add(manifest_bytes.len() as u64))
            .and_then(|size| size.checked_add(payload_total))
            .ok_or_else(|| {
                BackupError::InvalidRequest("The macOS export size cannot be represented.".into())
            })?;
        if archive_size > self.max_archive_bytes {
            return Err(BackupError::InvalidRequest(
                "The selected macOS exports exceed the archive size safety limit.".into(),
            ));
        }
        let size_bytes = write_immutable_package(
            destination,
            &mut manifest,
            payloads,
            cancellation,
            token,
            self.free_space,
        )?;
        drop(staged_payload_files);
        Ok(BackupResult {
            created_at_ms: manifest.created_at_ms,
            destination: destination.to_path_buf(),
            size_bytes,
            computers: manifest
                .computers
                .iter()
                .map(|computer| computer.name.clone())
                .collect(),
        })
    }

    /// Determine the format-4 runtime discriminant after strict header and
    /// manifest validation. Payload bytes are not read.
    pub(crate) fn detect_archive_kind(
        &self,
        archive: &Path,
        cancellation: &Cancellation,
    ) -> Result<ArchiveRuntimeKind, BackupError> {
        let package = read_and_verify_package(
            archive,
            self.max_archive_bytes,
            cancellation,
            super::PayloadMode::Describe,
        )?;
        match package.manifest.runtime.name.as_str() {
            "microsandbox" => Ok(ArchiveRuntimeKind::MicroSandbox),
            RUNTIME_NAME => Ok(ArchiveRuntimeKind::AppleVirtualization),
            "silo-computers" => Ok(ArchiveRuntimeKind::Mixed),
            _ => Err(BackupError::UnsupportedArchive(
                "This export uses an unsupported computer runtime.".into(),
            )),
        }
    }

    /// Verify every row in the archive and extract the selected component roles
    /// to an owner-only staging directory on the destination filesystem.
    pub(crate) fn extract_macos_components(
        &self,
        archive: &Path,
        source_name: Option<&str>,
        roles: &[MacosComponentRole],
        stage_parent: &Path,
        cancellation: &Cancellation,
    ) -> Result<PreparedMacosArchive, BackupError> {
        let _operation = self.begin()?;
        check_cancelled(cancellation)?;
        if roles.is_empty() {
            return Err(BackupError::InvalidRequest(
                "Choose at least one macOS component to import.".into(),
            ));
        }
        let mut role_set = HashSet::new();
        if roles.iter().any(|role| !role_set.insert(*role)) {
            return Err(BackupError::InvalidRequest(
                "The requested macOS component roles contain duplicates.".into(),
            ));
        }
        let stage_parent_metadata = fs::symlink_metadata(stage_parent)?;
        if !stage_parent_metadata.is_dir() || stage_parent_metadata.file_type().is_symlink() {
            return Err(BackupError::InvalidRequest(
                "The macOS import staging parent must be a real directory.".into(),
            ));
        }
        let stage = tempfile::Builder::new()
            .prefix("restore-macos-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(stage_parent)?;
        let stage_free = (self.free_space)(stage.path())?;
        if fs::metadata(stage.path())?.permissions().mode() & 0o777 != 0o700 {
            return Err(BackupError::InvalidRequest(
                "The macOS import staging directory is not owner-only.".into(),
            ));
        }
        let package = read_and_verify_package(
            archive,
            self.max_archive_bytes,
            cancellation,
            super::PayloadMode::ExtractMacos {
                dir: stage.path(),
                source: source_name,
                roles,
                stage_free,
            },
        )?;
        if package.manifest.runtime.name != RUNTIME_NAME
            && package.manifest.runtime.name != "silo-computers"
        {
            return Err(BackupError::InvalidRequest(
                "The selected export is not an Apple Virtualization archive.".into(),
            ));
        }
        let extracted = package.macos_extracted.ok_or_else(|| {
            BackupError::InvalidArchive("the selected components were not extracted".into())
        })?;
        let computer = package
            .manifest
            .computers
            .get(extracted.index)
            .ok_or_else(|| {
                BackupError::InvalidArchive("the selected computer is missing".into())
            })?;
        let runtime_config = if package.manifest.runtime.name == "silo-computers" {
            super::mixed_native_configuration(computer)?
        } else {
            computer.runtime_config.clone()
        };
        let config = decode_configuration(&runtime_config)?;
        Ok(PreparedMacosArchive {
            source_name: computer.name.clone(),
            config,
            computer_configuration: computer.computer_configuration.clone(),
            component_paths: extracted.component_paths,
            _stage: stage,
        })
    }
}

pub(super) fn validate_runtime(runtime: &RuntimeManifest) -> Result<(), BackupError> {
    if runtime.version != RUNTIME_VERSION
        || runtime.snapshot_format != SNAPSHOT_FORMAT
        || runtime.guest_architecture != "aarch64"
    {
        return Err(BackupError::UnsupportedArchive(
            "This Apple Virtualization export uses an unsupported component version or architecture.".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_computer(
    name: &str,
    runtime_config: &Value,
    computer_configuration: &Value,
    payload_size: u64,
    volumes: &[Value],
) -> Result<(), BackupError> {
    let config = decode_configuration(runtime_config)?;
    validate_configuration(&config, name)?;
    validate_computer_configuration(computer_configuration)?;
    if !volumes.is_empty() {
        return Err(invalid_archive("Apple Virtualization archives cannot contain Linux volumes"));
    }
    if stream_size(&config)? != payload_size {
        return Err(invalid_archive("the component payload length does not match its manifest"));
    }
    Ok(())
}

pub(super) fn verify_payloads(
    archive: &mut File,
    manifest: &PackageManifest,
    selected: Option<(usize, &[MacosComponentRole])>,
    stage: Option<&Path>,
    stage_free: Option<u64>,
    cancellation: &Cancellation,
) -> Result<Option<BTreeMap<MacosComponentRole, PathBuf>>, BackupError> {
    let selected_paths = match (selected, stage, stage_free) {
        (Some((index, roles)), Some(_), Some(stage_free)) => {
            let config = manifest
                .computers
                .get(index)
                .ok_or_else(|| invalid_archive("the selected computer is missing"))?;
            let config = decode_configuration(&config.runtime_config)?;
            let available = config
                .components
                .iter()
                .map(|component| component.role)
                .collect::<HashSet<_>>();
            if roles.iter().any(|role| !available.contains(role)) {
                return Err(BackupError::InvalidRequest(
                    "The selected export does not contain every requested component.".into(),
                ));
            }
            let required = config
                .components
                .iter()
                .filter(|component| roles.contains(&component.role))
                .try_fold(FREE_SPACE_RESERVE, |required, component| {
                    required.checked_add(component.logical_length)
                })
                .ok_or_else(|| invalid_archive("selected component sizes overflow"))?;
            if stage_free < required {
                return Err(BackupError::InsufficientSpace(format!(
                    "Importing {} needs {} of free space for staged macOS components; {} is available.",
                    manifest.computers[index].name,
                    format_bytes(required),
                    format_bytes(stage_free)
                )));
            }
            Some(BTreeMap::new())
        }
        (None, None, None) => None,
        _ => return Err(invalid_archive("the component extraction request is inconsistent")),
    };
    let mut selected_paths = selected_paths;

    for (index, computer) in manifest.computers.iter().enumerate() {
        check_cancelled(cancellation)?;
        let config = decode_configuration(&computer.runtime_config)?;
        let mut payload = HashingLimitReader::new(archive, computer.payload_size, cancellation);
        read_stream_header(&mut payload, &config, cancellation)?;
        let should_extract = selected.is_some_and(|(selected_index, _)| selected_index == index);
        let requested_roles = selected
            .filter(|(selected_index, _)| *selected_index == index)
            .map(|(_, roles)| roles);
        let mut paths = BTreeMap::new();
        for component in &config.components {
            check_cancelled(cancellation)?;
            let mut output = if should_extract
                && requested_roles.is_some_and(|roles| roles.contains(&component.role))
            {
                let stage = stage.expect("selected extraction has a stage");
                Some(create_component_file(stage, component.role)?)
            } else {
                None
            };
            verify_component_frame(
                &mut payload,
                component,
                output.as_mut().map(|(_, file)| file),
                cancellation,
            )?;
            if let Some((path, file)) = output {
                paths.insert(component.role, path);
                drop(file);
            }
        }
        if payload.remaining != 0 {
            return Err(invalid_archive("the component payload has extra or missing bytes"));
        }
        let digest = format!("sha256:{:x}", payload.hasher.finalize());
        if digest != computer.payload_sha256 {
            return Err(invalid_archive("a computer payload failed its integrity check"));
        }
        if should_extract {
            selected_paths = Some(paths);
        }
    }
    Ok(selected_paths)
}

fn validate_config_base(config: &MacosArchiveConfig, row_name: &str) -> Result<(), String> {
    let source_id_valid = uuid::Uuid::parse_str(&config.source_computer_id)
        .is_ok_and(|id| id.to_string() == config.source_computer_id);
    if config.payload_version != 1
        || config.architecture != "aarch64"
        || config.source_name != row_name
        || !source_id_valid
        || !(MIN_CPUS..=MAX_CPUS).contains(&config.cpus)
        || !(MIN_MEMORY_GIB..=MAX_MEMORY_GIB).contains(&config.memory_gib)
        || !(MIN_DISK_GIB..=MAX_DISK_GIB).contains(&config.disk_gib)
        || !safe_text(&config.guest_version)
        || !safe_text(&config.guest_build)
        || !safe_text(&config.host_build)
        || !valid_digest(&config.hardware_model_digest)
        || !valid_fingerprint(&config.setup_fingerprint)
        || !valid_fingerprint(&config.computer_use_fingerprint)
    {
        return Err("the macOS archive configuration is invalid".into());
    }
    validate_source(&config.source)?;
    validate_memory(&config.memory)?;
    if let MacosArchiveMemory::Saved {
        configuration_digest,
        ..
    } = &config.memory
    {
        let expected = memory_configuration_digest(
            config.cpus,
            config.memory_gib,
            config.disk_gib,
            &config.hardware_model_digest,
        )?;
        if configuration_digest != &expected {
            return Err("saved memory configuration digest does not match its archived computer".into());
        }
    }
    Ok(())
}

pub(crate) fn memory_configuration_digest(
    cpus: u64,
    memory_gib: u64,
    disk_gib: u64,
    hardware_model_digest: &str,
) -> Result<String, String> {
    let bytes = serde_json::to_vec(&serde_json::json!({
        "architecture": "aarch64",
        "cpus": cpus,
        "memoryGiB": memory_gib,
        "diskGiB": disk_gib,
        "hardwareModelDigest": hardware_model_digest,
    }))
    .map_err(|error| error.to_string())?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn validate_configuration(config: &MacosArchiveConfiguration, row_name: &str) -> Result<(), BackupError> {
    validate_config_base(&config.source_config(), row_name)
        .map_err(|_| invalid_archive("the macOS archive configuration is invalid"))?;
    let expected = expected_roles(&config.memory);
    if config.components.len() != expected.len()
        || config
            .components
            .iter()
            .map(|component| component.role)
            .ne(expected.iter().copied())
    {
        return Err(invalid_archive("the component roles or order are invalid"));
    }

    let mut logical_total = 0_u64;
    let mut stored_total = 0_u64;
    for component in &config.components {
        if component.logical_length == 0
            || component.stored_length == 0
            || component.logical_length > MAX_LOGICAL_COMPONENT_BYTES
            || component.stored_length > DEFAULT_MAX_ARCHIVE_BYTES
            || !valid_digest(&component.sha256)
        {
            return Err(invalid_archive("a component length or digest is invalid"));
        }
        logical_total = logical_total
            .checked_add(component.logical_length)
            .filter(|size| *size <= MAX_LOGICAL_COMPONENT_BYTES)
            .ok_or_else(|| invalid_archive("component logical lengths overflow"))?;
        stored_total = stored_total
            .checked_add(component.stored_length)
            .filter(|size| *size <= DEFAULT_MAX_ARCHIVE_BYTES)
            .ok_or_else(|| invalid_archive("component stored lengths overflow"))?;
    }

    let disk = component(config, MacosComponentRole::Disk)?;
    if disk.logical_length != config.disk_gib.checked_mul(GIB).unwrap_or(0) {
        return Err(invalid_archive("the disk component size does not match diskGiB"));
    }
    let model = component(config, MacosComponentRole::HardwareModel)?;
    if model.sha256 != config.hardware_model_digest {
        return Err(invalid_archive("the hardware model digest does not match its component"));
    }
    match &config.memory {
        MacosArchiveMemory::None => {}
        MacosArchiveMemory::Saved { size, sha256, .. } => {
            let saved = component(config, MacosComponentRole::Memory)?;
            if saved.logical_length != *size || saved.sha256 != *sha256 {
                return Err(invalid_archive("the saved memory metadata does not match its component"));
            }
        }
    }
    if logical_total == 0 || stored_total == 0 {
        return Err(invalid_archive("the component stream is empty"));
    }
    Ok(())
}

fn validate_source(source: &MacosArchiveSourceKind) -> Result<(), String> {
    match source {
        MacosArchiveSourceKind::WholeComputer => Ok(()),
        MacosArchiveSourceKind::Checkpoint { id, name, .. } => {
            if !uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == *id)
                || !safe_text(name)
            {
                return Err("checkpoint source metadata is invalid".into());
            }
            Ok(())
        }
    }
}

fn validate_memory(memory: &MacosArchiveMemory) -> Result<(), String> {
    if let MacosArchiveMemory::Saved {
        host_build,
        configuration_digest,
        size,
        sha256,
    } = memory
    {
        if !safe_text(host_build)
            || !valid_digest(configuration_digest)
            || *size == 0
            || *size > MAX_LOGICAL_COMPONENT_BYTES
            || !valid_digest(sha256)
        {
            return Err("saved memory metadata is invalid".into());
        }
    }
    Ok(())
}

fn validate_computer_configuration(value: &Value) -> Result<(), BackupError> {
    if value != &Value::Object(serde_json::Map::new()) {
        return Err(invalid_archive(
            "Apple Virtualization computerConfiguration must be an empty object",
        ));
    }
    Ok(())
}

fn expected_roles(memory: &MacosArchiveMemory) -> Vec<MacosComponentRole> {
    let mut roles = vec![
        MacosComponentRole::Disk,
        MacosComponentRole::Auxiliary,
        MacosComponentRole::HardwareModel,
        MacosComponentRole::MachineIdentifier,
    ];
    if matches!(memory, MacosArchiveMemory::Saved { .. }) {
        roles.push(MacosComponentRole::Memory);
    }
    roles
}

fn component(
    config: &MacosArchiveConfiguration,
    role: MacosComponentRole,
) -> Result<&MacosArchiveComponent, BackupError> {
    config
        .components
        .iter()
        .find(|component| component.role == role)
        .ok_or_else(|| invalid_archive("a required component is missing"))
}

fn decode_configuration(value: &Value) -> Result<MacosArchiveConfiguration, BackupError> {
    serde_json::from_value(value.clone())
        .map_err(|_| invalid_archive("the macOS runtime configuration is malformed"))
}

pub(super) fn stream_size(config: &MacosArchiveConfiguration) -> Result<u64, BackupError> {
    let index_size = (config.components.len() as u64)
        .checked_mul(STREAM_INDEX_ENTRY_BYTES)
        .and_then(|size| size.checked_add(STREAM_FIXED_HEADER_BYTES))
        .ok_or_else(|| invalid_archive("the component index size overflows"))?;
    config
        .components
        .iter()
        .try_fold(index_size, |sum, component| {
            sum.checked_add(component.stored_length)
        })
        .ok_or_else(|| invalid_archive("the component stream size overflows"))
}

fn encode_component_stream(
    output: &mut File,
    inputs: Vec<PreparedInput>,
    source: MacosArchiveConfig,
    row_name: &str,
    cancellation: &Cancellation,
) -> Result<MacosArchiveConfiguration, BackupError> {
    let roles = expected_roles(&source.memory);
    if inputs.len() != roles.len()
        || inputs
            .iter()
            .map(|input| input.role)
            .ne(roles.iter().copied())
    {
        return Err(BackupError::InvalidRequest(
            "The macOS export components are not in the required order.".into(),
        ));
    }

    let placeholder_components = inputs
        .iter()
        .map(|input| MacosArchiveComponent {
            role: input.role,
            logical_length: input.logical_length,
            stored_length: 0,
            sha256: format!("sha256:{}", "0".repeat(64)),
        })
        .collect::<Vec<_>>();
    write_stream_header(output, &placeholder_components)?;
    let mut components = Vec::with_capacity(inputs.len());
    let mut buffer = [0_u8; 128 * 1024];
    for mut input in inputs {
        check_cancelled(cancellation)?;
        let frame_start = output.stream_position()?;
        let mut encoder = zstd::stream::write::Encoder::new(&mut *output, 3)?;
        let mut hasher = Sha256::new();
        let mut copied = 0_u64;
        loop {
            check_cancelled(cancellation)?;
            let count = input.file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            copied = copied.checked_add(count as u64).ok_or_else(|| {
                BackupError::InvalidRequest("A macOS component size overflows.".into())
            })?;
            if copied > input.logical_length {
                return Err(BackupError::InvalidRequest(
                    "A macOS component changed while it was being exported.".into(),
                ));
            }
            hasher.update(&buffer[..count]);
            encoder.write_all(&buffer[..count])?;
        }
        let _ = encoder.finish()?;
        if copied != input.logical_length {
            return Err(BackupError::InvalidRequest(
                "A macOS component changed while it was being exported.".into(),
            ));
        }
        let frame_end = output.stream_position()?;
        let stored_length = frame_end
            .checked_sub(frame_start)
            .ok_or_else(|| invalid_archive("the component frame length is invalid"))?;
        if stored_length == 0 {
            return Err(invalid_archive("a component zstd frame is empty"));
        }
        components.push(MacosArchiveComponent {
            role: input.role,
            logical_length: input.logical_length,
            stored_length,
            sha256: format!("sha256:{:x}", hasher.finalize()),
        });
    }
    let end = output.stream_position()?;
    let config = MacosArchiveConfiguration::from_source(source, components);
    validate_configuration(&config, row_name)?;
    output.seek(SeekFrom::Start(0))?;
    write_stream_header(output, &config.components)?;
    output.seek(SeekFrom::Start(end))?;
    Ok(config)
}

fn write_stream_header(
    output: &mut impl Write,
    components: &[MacosArchiveComponent],
) -> Result<(), BackupError> {
    output.write_all(STREAM_MAGIC)?;
    output.write_all(&STREAM_VERSION.to_be_bytes())?;
    let count = u32::try_from(components.len())
        .map_err(|_| invalid_archive("the component count overflows"))?;
    output.write_all(&count.to_be_bytes())?;
    for component in components {
        output.write_all(&component.role.index().to_be_bytes())?;
        output.write_all(&component.logical_length.to_be_bytes())?;
        output.write_all(&component.stored_length.to_be_bytes())?;
        output.write_all(&digest_bytes(&component.sha256).ok_or_else(|| {
            invalid_archive("a component digest cannot be encoded")
        })?)?;
    }
    Ok(())
}

fn read_stream_header(
    input: &mut impl Read,
    config: &MacosArchiveConfiguration,
    cancellation: &Cancellation,
) -> Result<(), BackupError> {
    let mut magic = [0_u8; STREAM_MAGIC.len()];
    read_archive_exact(
        input,
        &mut magic,
        "the component stream header is incomplete",
        cancellation,
    )?;
    if &magic != STREAM_MAGIC {
        return Err(invalid_archive("the component stream header is not recognized"));
    }
    let version = read_archive_u32(input, cancellation)?;
    if version != STREAM_VERSION {
        return Err(BackupError::UnsupportedArchive(
            "This Apple Virtualization export uses an unsupported component stream version."
                .into(),
        ));
    }
    let count = read_archive_u32(input, cancellation)? as usize;
    if count != config.components.len() {
        return Err(invalid_archive("the component index count does not match its manifest"));
    }
    for expected in &config.components {
        let role = role_from_index(read_archive_u32(input, cancellation)?)
            .ok_or_else(|| invalid_archive("the component index contains an unknown role"))?;
        let logical_length = read_archive_u64(input, cancellation)?;
        let stored_length = read_archive_u64(input, cancellation)?;
        let mut digest = [0_u8; 32];
        read_archive_exact(
            input,
            &mut digest,
            "the component index is incomplete",
            cancellation,
        )?;
        if role != expected.role
            || logical_length != expected.logical_length
            || stored_length != expected.stored_length
            || Some(digest) != digest_bytes(&expected.sha256)
        {
            return Err(invalid_archive("the component index does not match its manifest"));
        }
    }
    Ok(())
}

fn verify_component_frame(
    payload: &mut HashingLimitReader<'_>,
    component: &MacosArchiveComponent,
    mut output: Option<&mut File>,
    cancellation: &Cancellation,
) -> Result<(), BackupError> {
    let bounded = (&mut *payload).take(component.stored_length);
    let buffered = BufReader::with_capacity(128 * 1024, bounded);
    let mut decoder = zstd::stream::read::Decoder::with_buffer(buffered)
        .map_err(|_| invalid_archive("a component zstd frame is malformed"))?;
    decoder
        .window_log_max(MAX_WINDOW_LOG)
        .map_err(|_| invalid_archive("a component exceeds the 128 MiB zstd window limit"))?;
    decoder = decoder.single_frame();

    let mut hasher = Sha256::new();
    let mut expanded = 0_u64;
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        check_cancelled(cancellation)?;
        let count = decoder
            .read(&mut buffer)
            .map_err(|_| frame_error(cancellation, "a component zstd frame is truncated or invalid"))?;
        if count == 0 {
            break;
        }
        expanded = expanded
            .checked_add(count as u64)
            .ok_or_else(|| invalid_archive("a component expanded length overflows"))?;
        if expanded > component.logical_length {
            return Err(invalid_archive("a component expands beyond its declared length"));
        }
        hasher.update(&buffer[..count]);
        if let Some(output) = output.as_deref_mut() {
            if buffer[..count].iter().all(|byte| *byte == 0) {
                output.seek(SeekFrom::Current(count as i64))?;
            } else {
                output.write_all(&buffer[..count])?;
            }
        }
    }
    decoder
        .finish_frame()
        .map_err(|_| frame_error(cancellation, "a component zstd frame is truncated or invalid"))?;
    let mut remaining = decoder.finish();
    let trailing = remaining
        .fill_buf()
        .map_err(|_| frame_error(cancellation, "a component frame has unread compressed data"))?;
    if !trailing.is_empty() || remaining.get_ref().limit() != 0 {
        return Err(invalid_archive("a component contains extra zstd frames or bytes"));
    }

    if expanded != component.logical_length {
        return Err(invalid_archive("a component does not expand to its declared length"));
    }
    let digest = format!("sha256:{:x}", hasher.finalize());
    if digest != component.sha256 {
        return Err(invalid_archive("a component failed its logical integrity check"));
    }
    if let Some(output) = output {
        output.set_len(component.logical_length)?;
        output.sync_all()?;
    }
    Ok(())
}

fn create_component_file(
    stage: &Path,
    role: MacosComponentRole,
) -> Result<(PathBuf, File), BackupError> {
    let path = stage.join(role.file_name());
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    if file.metadata()?.permissions().mode() & 0o777 != 0o600 {
        return Err(BackupError::InvalidRequest(
            "A staged macOS component is not owner-only.".into(),
        ));
    }
    Ok((path, file))
}

struct HashingLimitReader<'a> {
    input: &'a mut File,
    remaining: u64,
    hasher: Sha256,
    cancellation: &'a Cancellation,
}

impl<'a> HashingLimitReader<'a> {
    fn new(input: &'a mut File, limit: u64, cancellation: &'a Cancellation) -> Self {
        Self {
            input,
            remaining: limit,
            hasher: Sha256::new(),
            cancellation,
        }
    }
}

impl Read for HashingLimitReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.cancellation.cancelled() {
            return Err(io::Error::other("cancelled"));
        }
        if self.remaining == 0 || buffer.is_empty() {
            return Ok(0);
        }
        let read_len = buffer.len().min(self.remaining.min(usize::MAX as u64) as usize);
        let count = self.input.read(&mut buffer[..read_len])?;
        self.remaining -= count as u64;
        self.hasher.update(&buffer[..count]);
        Ok(count)
    }
}

fn read_archive_exact(
    input: &mut impl Read,
    buffer: &mut [u8],
    detail: &str,
    cancellation: &Cancellation,
) -> Result<(), BackupError> {
    input.read_exact(buffer).map_err(|_| {
        if cancellation.cancelled() {
            BackupError::Cancelled
        } else {
            invalid_archive(detail)
        }
    })
}

fn read_archive_u32(
    input: &mut impl Read,
    cancellation: &Cancellation,
) -> Result<u32, BackupError> {
    let mut bytes = [0_u8; 4];
    read_archive_exact(
        input,
        &mut bytes,
        "the component stream header is incomplete",
        cancellation,
    )?;
    Ok(u32::from_be_bytes(bytes))
}

fn read_archive_u64(
    input: &mut impl Read,
    cancellation: &Cancellation,
) -> Result<u64, BackupError> {
    let mut bytes = [0_u8; 8];
    read_archive_exact(
        input,
        &mut bytes,
        "the component index is incomplete",
        cancellation,
    )?;
    Ok(u64::from_be_bytes(bytes))
}

fn role_from_index(index: u32) -> Option<MacosComponentRole> {
    match index {
        1 => Some(MacosComponentRole::Disk),
        2 => Some(MacosComponentRole::Auxiliary),
        3 => Some(MacosComponentRole::HardwareModel),
        4 => Some(MacosComponentRole::MachineIdentifier),
        5 => Some(MacosComponentRole::Memory),
        _ => None,
    }
}

fn digest_bytes(value: &str) -> Option<[u8; 32]> {
    let digest = value.strip_prefix("sha256:")?;
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
        return None;
    }
    let mut output = [0_u8; 32];
    for (index, pair) in digest.as_bytes().chunks_exact(2).enumerate() {
        output[index] = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(output)
}

fn valid_digest(value: &str) -> bool {
    digest_bytes(value).is_some()
}

fn valid_fingerprint(value: &str) -> bool {
    value.len() == 16
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn safe_text(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TEXT_BYTES
        && !value.chars().any(char::is_control)
}

fn invalid_archive(detail: &str) -> BackupError {
    BackupError::InvalidArchive(detail.into())
}

fn frame_error(cancellation: &Cancellation, detail: &str) -> BackupError {
    if cancellation.cancelled() {
        BackupError::Cancelled
    } else {
        invalid_archive(detail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Cursor, Seek, SeekFrom},
        os::unix::fs::MetadataExt,
    };

    fn digest(bytes: &[u8]) -> String {
        format!("sha256:{:x}", Sha256::digest(bytes))
    }

    fn encoded_frame(bytes: &[u8]) -> Vec<u8> {
        zstd::stream::encode_all(bytes, 3).expect("compress test frame")
    }

    fn oversized_window_frame(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 3)
            .expect("create zstd test encoder");
        encoder.window_log(MAX_WINDOW_LOG + 1).unwrap();
        encoder.include_contentsize(false).unwrap();
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    fn verify_frame(
        frame: &[u8],
        logical: &[u8],
        declared_length: u64,
        declared_digest: &str,
    ) -> Result<(), BackupError> {
        let directory = tempfile::tempdir().expect("create temp directory");
        let path = directory.path().join("frame.zst");
        fs::write(&path, frame).expect("write frame");
        let mut archive = File::open(path).expect("open frame");
        let cancellation = Cancellation::default();
        let mut payload = HashingLimitReader::new(&mut archive, frame.len() as u64, &cancellation);
        verify_component_frame(
            &mut payload,
            &MacosArchiveComponent {
                role: MacosComponentRole::Disk,
                logical_length: declared_length,
                stored_length: frame.len() as u64,
                sha256: declared_digest.into(),
            },
            None,
            &cancellation,
        )?;
        assert_eq!(payload.remaining, 0);
        assert_eq!(
            format!("sha256:{:x}", payload.hasher.finalize()),
            digest(frame)
        );
        let _ = logical;
        Ok(())
    }

    fn valid_config() -> MacosArchiveConfiguration {
        let hardware_digest = digest(b"hardware-model");
        MacosArchiveConfiguration {
            payload_version: 1,
            architecture: "aarch64".into(),
            source_computer_id: "123e4567-e89b-12d3-a456-426614174000".into(),
            source_name: "Source Mac".into(),
            cpus: 4,
            memory_gib: 8,
            disk_gib: 32,
            guest_version: "15.0".into(),
            guest_build: "24A123".into(),
            host_build: "24A123".into(),
            hardware_model_digest: hardware_digest.clone(),
            setup_fingerprint: "0123456789abcdef".into(),
            computer_use_fingerprint: "fedcba9876543210".into(),
            source: MacosArchiveSourceKind::WholeComputer,
            memory: MacosArchiveMemory::None,
            components: vec![
                MacosArchiveComponent {
                    role: MacosComponentRole::Disk,
                    logical_length: 32 * GIB,
                    stored_length: 10,
                    sha256: digest(b"disk"),
                },
                MacosArchiveComponent {
                    role: MacosComponentRole::Auxiliary,
                    logical_length: 8,
                    stored_length: 10,
                    sha256: digest(b"auxiliary"),
                },
                MacosArchiveComponent {
                    role: MacosComponentRole::HardwareModel,
                    logical_length: 13,
                    stored_length: 10,
                    sha256: hardware_digest,
                },
                MacosArchiveComponent {
                    role: MacosComponentRole::MachineIdentifier,
                    logical_length: 16,
                    stored_length: 10,
                    sha256: digest(b"machine-id"),
                },
            ],
        }
    }

    fn valid_memory_config() -> MacosArchiveConfiguration {
        let mut config = valid_config();
        config.memory = MacosArchiveMemory::Saved {
            host_build: "25G83".into(),
            configuration_digest: memory_configuration_digest(
                config.cpus,
                config.memory_gib,
                config.disk_gib,
                &config.hardware_model_digest,
            )
            .unwrap(),
            size: 6,
            sha256: digest(b"memory"),
        };
        config.components.push(MacosArchiveComponent {
            role: MacosComponentRole::Memory,
            logical_length: 6,
            stored_length: 10,
            sha256: digest(b"memory"),
        });
        config
    }

    #[test]
    fn saved_memory_configuration_digest_must_match_archived_resources_and_model() {
        let valid = valid_memory_config();
        validate_configuration(&valid, &valid.source_name).unwrap();

        let mut invalid = valid;
        if let MacosArchiveMemory::Saved {
            configuration_digest,
            ..
        } = &mut invalid.memory
        {
            *configuration_digest = format!("sha256:{}", "1".repeat(64));
        }
        assert!(validate_config_base(&invalid.source_config(), &invalid.source_name)
            .unwrap_err()
            .contains("configuration digest"));
        assert!(validate_configuration(&invalid, &invalid.source_name).is_err());
    }

    #[test]
    fn component_frame_verifies_logical_bytes_and_restores_zero_ranges_sparse() {
        let logical = vec![0_u8; 1024 * 1024];
        let frame = encoded_frame(&logical);
        let directory = tempfile::tempdir().expect("create temp directory");
        let archive_path = directory.path().join("frame.zst");
        fs::write(&archive_path, &frame).expect("write frame");
        let mut archive = File::open(archive_path).expect("open frame");
        let cancellation = Cancellation::default();
        let mut payload = HashingLimitReader::new(&mut archive, frame.len() as u64, &cancellation);
        let (output_path, mut output) =
            create_component_file(directory.path(), MacosComponentRole::Disk).unwrap();
        verify_component_frame(
            &mut payload,
            &MacosArchiveComponent {
                role: MacosComponentRole::Disk,
                logical_length: logical.len() as u64,
                stored_length: frame.len() as u64,
                sha256: digest(&logical),
            },
            Some(&mut output),
            &cancellation,
        )
        .unwrap();
        assert_eq!(output.metadata().unwrap().mode() & 0o777, 0o600);
        assert_eq!(output.metadata().unwrap().len(), logical.len() as u64);
        assert!(output.metadata().unwrap().blocks() * 512 < logical.len() as u64);
        drop(output);
        let restored = fs::read(output_path).unwrap();
        assert_eq!(restored, logical);
        assert!(restored.iter().all(|byte| *byte == 0));
    }

    #[test]
    fn component_frame_rejects_truncation_extra_frames_bombs_and_bad_hashes() {
        let logical = b"a bounded component";
        let frame = encoded_frame(logical);
        let mut truncated = frame.clone();
        truncated.pop();
        assert!(matches!(
            verify_frame(&truncated, logical, logical.len() as u64, &digest(logical)),
            Err(BackupError::InvalidArchive(_))
        ));

        let mut extra_frame = frame.clone();
        extra_frame.extend(encoded_frame(b"unindexed second frame"));
        assert!(matches!(
            verify_frame(&extra_frame, logical, logical.len() as u64, &digest(logical)),
            Err(BackupError::InvalidArchive(_))
        ));

        assert!(matches!(
            verify_frame(&frame, logical, 4, &digest(&logical[..4])),
            Err(BackupError::InvalidArchive(_))
        ));
        assert!(matches!(
            verify_frame(&frame, logical, logical.len() as u64, &digest(b"other")),
            Err(BackupError::InvalidArchive(_))
        ));

        let large_window = oversized_window_frame(logical);
        assert!(matches!(
            verify_frame(
                &large_window,
                logical,
                logical.len() as u64,
                &digest(logical)
            ),
            Err(BackupError::InvalidArchive(_))
        ));
    }

    #[test]
    fn component_index_is_fixed_order_and_matches_the_manifest() {
        let config = valid_config();
        validate_configuration(&config, "Source Mac").unwrap();
        let mut encoded = Cursor::new(Vec::new());
        write_stream_header(&mut encoded, &config.components).unwrap();
        encoded.seek(SeekFrom::Start(0)).unwrap();
        let cancellation = Cancellation::default();
        read_stream_header(&mut encoded, &config, &cancellation).unwrap();

        let mut wrong_order = config.clone();
        wrong_order.components.swap(0, 1);
        assert!(matches!(
            validate_configuration(&wrong_order, "Source Mac"),
            Err(BackupError::InvalidArchive(_))
        ));

        let mut wrong_digest = Cursor::new(Vec::new());
        let mut mismatched_components = config.components.clone();
        mismatched_components[0].sha256 = digest(b"different disk");
        write_stream_header(&mut wrong_digest, &mismatched_components).unwrap();
        wrong_digest.seek(SeekFrom::Start(0)).unwrap();
        assert!(matches!(
            read_stream_header(&mut wrong_digest, &config, &cancellation),
            Err(BackupError::InvalidArchive(_))
        ));
    }

    #[test]
    fn runtime_and_configuration_reject_unknown_versions_and_fields() {
        let mut runtime = RuntimeManifest {
            name: RUNTIME_NAME.into(),
            version: RUNTIME_VERSION.into(),
            snapshot_format: SNAPSHOT_FORMAT.into(),
            guest_architecture: "aarch64".into(),
        };
        validate_runtime(&runtime).unwrap();
        runtime.version = "2".into();
        assert!(matches!(
            validate_runtime(&runtime),
            Err(BackupError::UnsupportedArchive(_))
        ));

        let mut config = serde_json::to_value(valid_config()).unwrap();
        config
            .as_object_mut()
            .unwrap()
            .insert("unexpected".into(), Value::Bool(true));
        assert!(matches!(
            decode_configuration(&config),
            Err(BackupError::InvalidArchive(_))
        ));
    }

    #[test]
    fn avf_manifest_requires_empty_legacy_configuration_and_exact_payload_size() {
        let config = valid_config();
        let runtime_config = serde_json::to_value(&config).unwrap();
        let payload_size = stream_size(&config).unwrap();
        validate_computer(
            "Source Mac",
            &runtime_config,
            &serde_json::json!({}),
            payload_size,
            &[],
        )
        .unwrap();
        assert!(matches!(
            validate_computer(
                "Source Mac",
                &runtime_config,
                &serde_json::json!({"macAddress":"00:11:22:33:44:55"}),
                payload_size,
                &[],
            ),
            Err(BackupError::InvalidArchive(_))
        ));
        assert!(matches!(
            validate_computer(
                "Source Mac",
                &runtime_config,
                &serde_json::json!({}),
                payload_size + 1,
                &[],
            ),
            Err(BackupError::InvalidArchive(_))
        ));
    }

    #[test]
    fn failed_component_extraction_removes_its_private_stage() {
        let temp = tempfile::tempdir().expect("create temp directory");
        let archive = temp.path().join("invalid.silo-backup");
        fs::write(&archive, b"not an archive").expect("write invalid archive");
        let service = BackupService::with_runner(
            super::super::MsbCommand {
                executable: temp.path().join("msb"),
                home: temp.path().join("home"),
                storage_home: None,
                library: temp.path().join("libkrunfw"),
            },
            temp.path().join("scratch"),
            super::super::SystemMsbRunner,
        );

        assert!(matches!(
            service.extract_macos_components(
                &archive,
                None,
                &[MacosComponentRole::Disk],
                temp.path(),
                &Cancellation::default(),
            ),
            Err(BackupError::InvalidArchive(_))
        ));
        let children = fs::read_dir(temp.path())
            .expect("read stage parent")
            .map(|entry| entry.expect("read stage entry").file_name())
            .collect::<Vec<_>>();
        assert_eq!(children, [std::ffi::OsStr::new("invalid.silo-backup")]);
    }

    #[test]
    fn component_extraction_checks_free_space_before_expanding_payloads() {
        let temp = tempfile::tempdir().expect("create temporary directory");
        let archive = temp.path().join("oversized-components.silo-backup");
        let mut config = valid_config();
        config.source_name = "source-mac".into();
        let runtime_config = serde_json::to_value(&config).unwrap();
        let payload_size = stream_size(&config).unwrap();
        // The frame bytes are intentionally invalid. The space preflight must
        // reject the 32 GiB logical disk before trying to read or expand them.
        let payload = vec![0_u8; payload_size as usize];
        let manifest = PackageManifest {
            schema_version: FORMAT_VERSION,
            created_at_ms: 1,
            runtime: RuntimeManifest {
                name: RUNTIME_NAME.into(),
                version: RUNTIME_VERSION.into(),
                snapshot_format: SNAPSHOT_FORMAT.into(),
                guest_architecture: "aarch64".into(),
            },
            computers: vec![PackageComputer {
                name: config.source_name.clone(),
                runtime_config,
                computer_configuration: serde_json::json!({}),
                payload_size,
                payload_sha256: digest(&payload),
                volumes: Vec::new(),
            }],
        };
        let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
        let mut file = File::create(&archive).unwrap();
        file.write_all(MAGIC).unwrap();
        file.write_all(&FORMAT_VERSION.to_be_bytes()).unwrap();
        file.write_all(&(manifest_bytes.len() as u64).to_be_bytes())
            .unwrap();
        file.write_all(&manifest_bytes).unwrap();
        file.write_all(&payload).unwrap();

        let mut service = BackupService::with_runner(
            super::super::MsbCommand {
                executable: temp.path().join("msb"),
                home: temp.path().join("home"),
                storage_home: None,
                library: temp.path().join("libkrunfw"),
            },
            temp.path().join("scratch"),
            super::super::SystemMsbRunner,
        );
        service.free_space = |_| Ok(FREE_SPACE_RESERVE + 16 * 1024 * 1024);

        let Err(error) = service.extract_macos_components(
            &archive,
            Some("source-mac"),
            &[MacosComponentRole::Disk],
            temp.path(),
            &Cancellation::default(),
        ) else {
            panic!("extraction ignored the insufficient free-space budget");
        };
        assert!(matches!(error, BackupError::InsufficientSpace(_)), "{error}");
        let children = fs::read_dir(temp.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(children, [std::ffi::OsStr::new("oversized-components.silo-backup")]);
    }
}
