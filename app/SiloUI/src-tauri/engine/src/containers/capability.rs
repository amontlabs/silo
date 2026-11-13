//! What a container computer cannot do: one list, one check.
//!
//! The desktop installer reaches the runtime by its own path (not the argument shape
//! [`super::route`] answers), so a container would otherwise be handed to an installer that does
//! not know it. Every such entry point names its [`Capability`] and calls [`require`] before it
//! does anything; the app hides the same actions from [`unavailable`]. A container computer has
//! everything a virtual machine has but the memory in its checkpoints.

use super::layout::Layout;
use crate::runtime::RuntimePaths;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    /// The graphical desktop and what runs on it.
    Desktop,
    /// A checkpoint that carries the memory and running processes. A container's checkpoints are
    /// disks only.
    MemorySnapshots,
}

impl Capability {
    pub const ALL: [Capability; 2] = [Capability::Desktop, Capability::MemorySnapshots];

    /// The name the app uses for the action.
    pub const fn id(self) -> &'static str {
        match self {
            Capability::Desktop => "desktop",
            Capability::MemorySnapshots => "memorySnapshots",
        }
    }

    /// The verb of the refusal: "Silo cannot <verb> a container computer yet."
    pub const fn verb(self) -> &'static str {
        match self {
            Capability::Desktop => "give a desktop to",
            Capability::MemorySnapshots => "save the memory of",
        }
    }

    /// Whether the container kind does this. A checkpoint never carries a container's memory.
    pub const fn container_has(self) -> bool {
        matches!(self, Capability::Desktop)
    }
}

/// The refusal for a container computer, for any verb (also a runtime command it cannot answer).
pub fn refusal(verb: &str) -> String {
    format!("Silo cannot {verb} a container computer yet. Use a virtual machine computer for this.")
}

/// Whether the runtime home holds a container computer called `name`.
pub fn is_container(home: &Path, name: &str) -> bool {
    crate::runtime::validate_name(name).is_ok() && Layout::new(home).computer(name).exists()
}

/// Whether the computer `name` can save its memory in a checkpoint: every computer but a container.
pub fn memory_snapshots(home: &Path, name: &str) -> bool {
    Capability::MemorySnapshots.container_has() || !is_container(home, name)
}

/// Refuses `capability` for `name` when it is a container computer that lacks it. Call it before
/// any side effect: no command run, no setting saved, no file written.
pub fn require_in(home: &Path, name: &str, capability: Capability) -> Result<(), String> {
    if !capability.container_has() && is_container(home, name) {
        return Err(refusal(capability.verb()));
    }
    Ok(())
}

pub fn require(paths: &RuntimePaths, name: &str, capability: Capability) -> Result<(), String> {
    require_in(&paths.home, name, capability)
}

/// What the app hides on a container computer.
pub fn unavailable() -> Vec<&'static str> {
    Capability::ALL
        .into_iter()
        .filter(|capability| !capability.container_has())
        .map(Capability::id)
        .collect()
}

/// A container computer's record in a runtime home, for tests of the entry points.
#[cfg(test)]
pub(crate) fn make_container(home: &Path, name: &str) {
    let dir = Layout::new(home).computer(name);
    std::fs::create_dir_all(dir.path()).unwrap();
    dir.write_record(&super::layout::ComputerRecord {
        schema_version: super::layout::RECORD_VERSION,
        name: name.into(),
        cpus: 1,
        max_cpus: 1,
        memory_mib: 1024,
        max_memory_mib: 1024,
        root_disk_mib: 1024,
        workspace_mib: 1024,
        labels: vec![("silo.managed".into(), "true".into())],
        env: Vec::new(),
        image_digest: "sha256:d".into(),
        image_reference: "r:1".into(),
        layers: Vec::new(),
        upper: "l0".into(),
        created_ms: 1,
        started_once: false,
        secrets: Vec::new(),
        github: false,
        network_blocked: false,
    })
    .unwrap();
}

#[cfg(test)]
mod tests {
    use super::make_container;
    use super::*;

    #[test]
    fn a_container_is_refused_every_capability_it_lacks_in_the_same_words() {
        let home = tempfile::tempdir().unwrap();
        make_container(home.path(), "box");
        for capability in Capability::ALL.into_iter().filter(|c| !c.container_has()) {
            let error = require_in(home.path(), "box", capability).unwrap_err();
            assert_eq!(
                error,
                format!(
                    "Silo cannot {} a container computer yet. Use a virtual machine computer for this.",
                    capability.verb()
                )
            );
        }
        assert!(require_in(home.path(), "box", Capability::Desktop).is_ok());
        assert_eq!(
            require_in(home.path(), "box", Capability::MemorySnapshots).unwrap_err(),
            "Silo cannot save the memory of a container computer yet. Use a virtual machine computer for this."
        );
    }

    #[test]
    fn a_virtual_machine_and_an_unknown_or_unsafe_name_pass() {
        let home = tempfile::tempdir().unwrap();
        make_container(home.path(), "box");
        for capability in Capability::ALL {
            assert!(require_in(home.path(), "vm", capability).is_ok());
            assert!(require_in(home.path(), "../box", capability).is_ok());
        }
    }

    #[test]
    fn the_app_hides_what_the_engine_refuses() {
        let ids = unavailable();
        for capability in Capability::ALL {
            assert_eq!(ids.contains(&capability.id()), !capability.container_has());
        }
        assert_eq!(ids, ["memorySnapshots"]);
        // The desktop, computer use and export: a container passes the check.
        assert!(Capability::Desktop.container_has());
        // The app reads the same list from this file.
        let app: Vec<String> = serde_json::from_str(include_str!(
            "../../../../src/features/computers/model/container-unavailable.json"
        ))
        .unwrap();
        assert_eq!(app, ids);
    }
}
