//! What a container computer cannot do yet: one list, one check.
//!
//! Terminal, editor, SSH access, export and the desktop reach the runtime by their own paths
//! (not the argument shape [`super::route`] answers), so a container would otherwise be handed to
//! MicroSandbox, or to an installer, that does not know it. Every such entry point names its
//! [`Capability`] and calls [`require`] before it does anything; the app hides the same actions
//! from [`unavailable`]. A later phase gives the container kind a capability by flipping
//! [`Capability::container_has`] and adding the code behind it.

use super::layout::Layout;
use crate::runtime::RuntimePaths;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    /// An interactive shell, locally or through a launcher.
    Terminal,
    /// An editor connection (the proxy and the key authorization behind it).
    Editor,
    /// A listener that serves SSH into the computer.
    SshAccess,
    /// A portable export or backup of the computer.
    Export,
    /// The graphical desktop and what runs on it.
    Desktop,
}

impl Capability {
    pub const ALL: [Capability; 5] = [
        Capability::Terminal,
        Capability::Editor,
        Capability::SshAccess,
        Capability::Export,
        Capability::Desktop,
    ];

    /// The name the app uses for the action.
    pub const fn id(self) -> &'static str {
        match self {
            Capability::Terminal => "terminal",
            Capability::Editor => "editor",
            Capability::SshAccess => "sshAccess",
            Capability::Export => "export",
            Capability::Desktop => "desktop",
        }
    }

    /// The verb of the refusal: "Silo cannot <verb> a container computer yet."
    pub const fn verb(self) -> &'static str {
        match self {
            Capability::Terminal => "open a terminal in",
            Capability::Editor => "connect an editor to",
            Capability::SshAccess => "serve SSH access to",
            Capability::Export => "export or back up",
            Capability::Desktop => "give a desktop to",
        }
    }

    /// Whether the container kind does this yet. Nothing is, until the phase that adds it.
    pub const fn container_has(self) -> bool {
        false
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
        labels: Vec::new(),
        env: Vec::new(),
        image_digest: "sha256:d".into(),
        image_reference: "r:1".into(),
        layers: Vec::new(),
        created_ms: 1,
        started_once: false,
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
        for capability in Capability::ALL {
            let error = require_in(home.path(), "box", capability).unwrap_err();
            assert_eq!(
                error,
                format!(
                    "Silo cannot {} a container computer yet. Use a virtual machine computer for this.",
                    capability.verb()
                )
            );
        }
        assert!(require_in(home.path(), "box", Capability::Terminal)
            .unwrap_err()
            .contains("cannot open a terminal in a container computer yet"));
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
        assert_eq!(
            ids,
            ["terminal", "editor", "sshAccess", "export", "desktop"]
        );
        // The app reads the same list from this file.
        let app: Vec<String> = serde_json::from_str(include_str!(
            "../../../../src/features/computers/model/container-unavailable.json"
        ))
        .unwrap();
        assert_eq!(app, ids);
    }
}
