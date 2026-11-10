//! What access a computer holds that a fork or a new computer could be handed: its secrets
//! and its GitHub access. Both stores are keyed by the computer's name.

use std::collections::BTreeSet;

use serde_json::Value;

/// A computer's GitHub access, as far as a read-only copy of it reaches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitHubAccess {
    /// `oauth` or `token`.
    pub method: String,
    pub all: bool,
    /// `owner/name`, in lowercase.
    pub repositories: BTreeSet<String>,
}

impl GitHubAccess {
    /// Whether a read-only copy of `other` reaches only what this access reaches.
    pub fn covers(&self, other: &GitHubAccess) -> bool {
        self.method == other.method
            && (self.all || (!other.all && other.repositories.is_subset(&self.repositories)))
    }

    /// Reads a saved GitHub assignment.
    pub fn from_assignment(assignment: &Value) -> Option<Self> {
        if !assignment.is_object() {
            return None;
        }
        let repositories = assignment["repositories"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|repository| repository["repository"].as_str())
            .map(str::to_ascii_lowercase)
            .collect();
        Some(Self {
            method: assignment["authenticationMethod"]
                .as_str()
                .unwrap_or("oauth")
                .to_owned(),
            all: assignment["repositoryMode"].as_str() == Some("all"),
            repositories,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Access {
    /// Names of the secrets assigned to the computer.
    pub secrets: BTreeSet<String>,
    pub github: Option<GitHubAccess>,
}

/// The access of the computer named `name`. An unreadable store counts as no access, so a
/// check against it asks rather than allows.
pub fn of_computer(name: &str) -> Access {
    let secrets = crate::secrets::assigned_names(name)
        .unwrap_or_default()
        .into_iter()
        .collect();
    let github = crate::github::export_computer(name)
        .ok()
        .and_then(|assignment| GitHubAccess::from_assignment(&assignment));
    Access { secrets, github }
}
