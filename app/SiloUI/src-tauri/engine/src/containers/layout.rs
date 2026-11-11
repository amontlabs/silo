//! Where a container computer's files live, and the record that describes it.
//!
//! ```text
//! <runtime home>/containers/
//!   images/<config digest>/rootfs        the pinned guest image, unpacked once per digest
//!   boot/<hash>/                         the shim layer: silo/init and an empty workspace
//!   computers/<name>/
//!     computer.json                      the record (ComputerRecord)
//!     work/                              the overlay work directory
//!   layers/<id>/                         the shared layer store: sealed layers and writable uppers
//!   snapshots/<group>/<name>.json        checkpoints (a chain of layer ids and a record template)
//!     bundle/config.json                 the OCI configuration crun runs
//!     state/                             crun's state, the holder's pid files, logs, sockets
//! ```
//!
//! The overlay's lower directories, top first, are the sealed layers newest to oldest, the shim
//! layer and the image: [`ComputerDir::lower_dirs`]. The writable layer is one of the store's
//! directories too (`ComputerRecord::upper`); a computer made before the store has it at
//! `computers/<name>/upper` and cannot be sealed.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const RECORD_VERSION: u8 = 1;

/// Added to the name of what is unpacked in the id mapping of [`super::idmap`], so a directory an
/// earlier build made with other file owners is never taken for it.
pub const OWNERSHIP: &str = "-keepid";

/// Removes the default ACL of a folder Silo keeps container files in, so nothing made below it
/// inherits one. A home folder with a default ACL (a shared server's, a runner's) would otherwise
/// give every layer an entry naming an account that a container's id mapping does not have, and
/// unpacking the image or an import then fails to restore it ("invalid argument"). A folder with
/// none, or a file system without ACLs, is left as it is.
#[cfg(target_os = "linux")]
pub fn strip_default_acl(dir: &Path) {
    use std::os::unix::ffi::OsStrExt;
    if let Ok(path) = std::ffi::CString::new(dir.as_os_str().as_bytes()) {
        // SAFETY: both arguments are valid NUL-terminated strings.
        unsafe { libc::removexattr(path.as_ptr(), c"system.posix_acl_default".as_ptr()) };
    }
}

#[cfg(not(target_os = "linux"))]
pub fn strip_default_acl(_: &Path) {}

#[derive(Clone, Debug)]
pub struct Layout {
    root: PathBuf,
}

impl Layout {
    pub fn new(runtime_home: &Path) -> Self {
        Self {
            root: runtime_home.join("containers"),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn images(&self) -> PathBuf {
        self.root.join("images")
    }

    /// The unpacked image with this config digest (`sha256:<hex>`).
    pub fn image(&self, digest: &str) -> PathBuf {
        self.images().join(format!(
            "{}{OWNERSHIP}",
            digest.trim_start_matches("sha256:")
        ))
    }

    pub fn boot(&self, hash: &str) -> PathBuf {
        self.root.join("boot").join(hash)
    }

    pub fn computers(&self) -> PathBuf {
        self.root.join("computers")
    }

    /// The shared layer store.
    pub fn layers(&self) -> PathBuf {
        self.root.join("layers")
    }

    pub fn layer(&self, id: &str) -> PathBuf {
        self.layers().join(id)
    }

    /// The checkpoints of the computers (and forks) of every group.
    pub fn snapshots(&self) -> PathBuf {
        self.root.join("snapshots")
    }

    pub fn computer(&self, name: &str) -> ComputerDir {
        ComputerDir {
            dir: self.computers().join(name),
            store: self.layers(),
        }
    }

    /// Names of the computers that have a record, sorted.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.computers())
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.path().join("computer.json").is_file())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect();
        names.sort();
        names
    }
}

#[derive(Clone, Debug)]
pub struct ComputerDir {
    dir: PathBuf,
    store: PathBuf,
}

impl ComputerDir {
    pub fn path(&self) -> &Path {
        &self.dir
    }

    pub fn record_path(&self) -> PathBuf {
        self.dir.join("computer.json")
    }

    pub fn exists(&self) -> bool {
        self.record_path().is_file()
    }

    /// Where a computer made before the layer store kept its writable layer.
    pub fn legacy_upper(&self) -> PathBuf {
        self.dir.join("upper")
    }

    /// The computer's writable layer.
    pub fn upper_of(&self, record: &ComputerRecord) -> PathBuf {
        if record.upper.is_empty() {
            self.legacy_upper()
        } else {
            self.store.join(&record.upper)
        }
    }

    pub fn work(&self) -> PathBuf {
        self.dir.join("work")
    }

    pub fn merged(&self) -> PathBuf {
        self.dir.join("merged")
    }

    pub fn layers(&self) -> PathBuf {
        self.dir.join("layers")
    }

    pub fn bundle(&self) -> PathBuf {
        self.dir.join("bundle")
    }

    pub fn state(&self) -> PathBuf {
        self.dir.join("state")
    }

    /// The overlay's lower directories, top first: the sealed layers newest to oldest, then
    /// the shim layer, then the image.
    pub fn lower_dirs(
        &self,
        record: &ComputerRecord,
        boot: &Path,
        image_rootfs: &Path,
    ) -> Vec<PathBuf> {
        let mut lower: Vec<PathBuf> = record
            .layers
            .iter()
            .rev()
            .map(|layer| self.store.join(layer))
            .collect();
        lower.push(boot.to_path_buf());
        lower.push(image_rootfs.to_path_buf());
        lower
    }

    pub fn read_record(&self) -> Result<ComputerRecord, String> {
        let text = std::fs::read_to_string(self.record_path())
            .map_err(|_| "The container computer's record could not be read.".to_owned())?;
        let record: ComputerRecord = serde_json::from_str(&text)
            .map_err(|_| "The container computer's record is damaged.".to_owned())?;
        if record.schema_version != RECORD_VERSION {
            return Err("This container computer was made by a newer Silo. Update Silo.".into());
        }
        Ok(record)
    }

    /// Reads the record, changes it and saves it, as one step: two changes (a start finishing,
    /// a secret assigned) never overwrite each other with a stale copy.
    pub fn update_record(
        &self,
        change: impl FnOnce(&mut ComputerRecord),
    ) -> Result<ComputerRecord, String> {
        static RECORDS: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = RECORDS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut record = self.read_record()?;
        change(&mut record);
        self.write_record(&record)?;
        Ok(record)
    }

    pub fn write_record(&self, record: &ComputerRecord) -> Result<(), String> {
        let text = serde_json::to_vec_pretty(record)
            .map_err(|_| "The container computer's record could not be saved.".to_owned())?;
        let temporary = self.dir.join("computer.json.new");
        std::fs::write(&temporary, text)
            .and_then(|()| std::fs::rename(&temporary, self.record_path()))
            .map_err(|_| "The container computer's record could not be saved.".to_owned())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComputerRecord {
    pub schema_version: u8,
    pub name: String,
    pub cpus: u64,
    pub max_cpus: u64,
    pub memory_mib: u64,
    pub max_memory_mib: u64,
    pub root_disk_mib: u64,
    pub workspace_mib: u64,
    pub labels: Vec<(String, String)>,
    pub env: Vec<(String, String)>,
    /// The config digest of the image the computer was made from.
    pub image_digest: String,
    pub image_reference: String,
    /// The ids of the sealed layers in the shared store, oldest first.
    pub layers: Vec<String>,
    /// The id of the writable layer in the store; empty for a computer made before the store.
    #[serde(default)]
    pub upper: String,
    pub created_ms: u64,
    /// Set after the first stop, so `inspect` can tell `Created` from `Stopped`.
    #[serde(default)]
    pub started_once: bool,
    /// The general secrets assigned to the computer: names and the hosts they may be sent to.
    /// Values live in the host credential store and reach the gateway only.
    #[serde(default)]
    pub secrets: Vec<SecretRecord>,
    /// Whether the computer carries the GitHub access secret (`SILO_GITHUB`), as a virtual
    /// machine does from creation.
    #[serde(default)]
    pub github: bool,
    /// Whether the computer's network is blocked (deny-all): a computer made from an import starts
    /// that way and stays so, as a virtual machine does, and its forks keep it. Otherwise the gateway
    /// applies its own policy (public addresses only).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub network_blocked: bool,
}

/// One assigned secret: its guest-visible name and where the gateway may substitute it.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SecretRecord {
    pub name: String,
    pub allowed_hosts: Vec<String>,
}

impl ComputerRecord {
    pub fn label(&self, key: &str) -> Option<&str> {
        self.labels
            .iter()
            .find(|(label, _)| label == key)
            .map(|(_, value)| value.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> ComputerRecord {
        ComputerRecord {
            schema_version: RECORD_VERSION,
            name: "dev".into(),
            cpus: 1,
            max_cpus: 1,
            memory_mib: 1024,
            max_memory_mib: 1024,
            root_disk_mib: 2048,
            workspace_mib: 2048,
            labels: vec![("silo.managed".into(), "true".into())],
            env: Vec::new(),
            image_digest: "sha256:abc".into(),
            image_reference: "ghcr.io/x/y:1".into(),
            layers: vec!["l1".into(), "l2".into()],
            upper: "l3".into(),
            created_ms: 5,
            started_once: false,
            secrets: Vec::new(),
            github: false,
            network_blocked: false,
        }
    }

    #[test]
    fn the_layer_chain_orders_the_lower_directories_top_first() {
        let layout = Layout::new(Path::new("/h"));
        let dir = layout.computer("dev");
        let lower = dir.lower_dirs(&record(), &layout.boot("b"), &layout.image("sha256:abc"));
        assert_eq!(
            lower,
            [
                "/h/containers/layers/l2",
                "/h/containers/layers/l1",
                "/h/containers/boot/b",
                "/h/containers/images/abc-keepid",
            ]
            .map(PathBuf::from)
        );
    }

    #[test]
    fn a_record_round_trips_and_names_list_only_computers_with_one() {
        let home = tempfile::tempdir().unwrap();
        let layout = Layout::new(home.path());
        let dir = layout.computer("dev");
        std::fs::create_dir_all(dir.path()).unwrap();
        std::fs::create_dir_all(layout.computer("half").path()).unwrap();
        assert!(!dir.exists());
        dir.write_record(&record()).unwrap();
        assert_eq!(dir.read_record().unwrap(), record());
        assert_eq!(layout.names(), ["dev"]);
        assert_eq!(record().label("silo.managed"), Some("true"));
    }

    #[test]
    fn a_record_from_a_newer_silo_is_refused() {
        let home = tempfile::tempdir().unwrap();
        let dir = Layout::new(home.path()).computer("dev");
        std::fs::create_dir_all(dir.path()).unwrap();
        let mut newer = record();
        newer.schema_version = 2;
        dir.write_record(&newer).unwrap();
        assert!(dir.read_record().unwrap_err().contains("newer Silo"));
    }
}
