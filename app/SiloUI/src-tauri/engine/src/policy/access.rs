//! What access a computer holds that a fork or a new computer could be handed: its secrets
//! and its GitHub access. Both stores are keyed by the computer's name.

use std::collections::BTreeSet;

use serde_json::Value;
use silo_api::methods::approvals::{
    ComputerAccessGitHub, ComputerAccessRepository, ComputerAccessSecret,
};

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

/// The secrets (names and hosts) and GitHub access of the computer named `name`, as
/// `computers.access` reports them. An unreadable store reads as nothing held.
pub fn held(name: &str) -> (Vec<ComputerAccessSecret>, Option<ComputerAccessGitHub>) {
    let secrets = crate::secrets::assigned_with_hosts(name)
        .unwrap_or_default()
        .into_iter()
        .map(|(name, hosts)| ComputerAccessSecret { name, hosts })
        .collect();
    let github = crate::github::export_computer(name)
        .ok()
        .and_then(|assignment| github_access(&assignment));
    (secrets, github)
}

/// A saved GitHub assignment as `computers.access` reports it.
pub fn github_access(assignment: &Value) -> Option<ComputerAccessGitHub> {
    if !assignment.is_object() {
        return None;
    }
    let repositories = assignment["repositories"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|repository| {
            Some(ComputerAccessRepository {
                repository: repository["repository"].as_str()?.to_owned(),
                write: repository["allowPushes"].as_bool().unwrap_or(false),
            })
        })
        .collect();
    let all = assignment["repositoryMode"].as_str() == Some("all");
    Some(ComputerAccessGitHub {
        method: assignment["authenticationMethod"]
            .as_str()
            .unwrap_or("oauth")
            .to_owned(),
        all_repositories: all,
        all_repositories_write: all
            && assignment["allRepositoriesAllowChanges"].as_bool() == Some(true),
        repositories,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn github_access_reports_repositories_and_write() {
        let selected = github_access(&json!({"repositoryMode": "selected", "repositories": [
            {"repository": "owner/app", "allowPushes": true},
            {"repository": "owner/docs", "allowPushes": false}]}))
        .unwrap();
        assert_eq!(selected.method, "oauth");
        assert!(!selected.all_repositories && !selected.all_repositories_write);
        assert_eq!(
            selected
                .repositories
                .iter()
                .map(|r| (r.repository.as_str(), r.write))
                .collect::<Vec<_>>(),
            [("owner/app", true), ("owner/docs", false)]
        );
        let all = github_access(
            &json!({"repositoryMode": "all", "authenticationMethod": "token",
            "allRepositoriesAllowChanges": true}),
        )
        .unwrap();
        assert!(all.all_repositories && all.all_repositories_write);
        assert_eq!(all.method, "token");
        assert!(github_access(&Value::Null).is_none());
    }
}
