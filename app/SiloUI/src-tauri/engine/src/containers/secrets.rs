//! Secrets and GitHub access of a container computer.
//!
//! The engine assigns secrets and GitHub access to a computer the same way for every kind: it
//! validates the material once (`secrets_runtime::validate_material`, the credential store),
//! then asks the runtime to `modify` the computer with `--secret NAME:passthrough=*@HOSTS` and
//! `--secret-rm NAME`, and checks the result by inspecting the computer. This module answers
//! the same commands for a container and reports the result in the shape MicroSandbox
//! inspects (`network.secrets`), so none of that policy is repeated here.
//!
//! What is kept: names and allowed hosts in the computer's record, never a value. Values reach
//! only the gateway (host side of the computer's network), which substitutes a placeholder
//! (`$MSB_<name>`) for the value in request headers to the allowed hosts after verifying the
//! upstream's TLS certificate. The guest sees the placeholder in its environment.

use super::{
    args::{Modify, SecretSpec, GITHUB_SECRET_NAME},
    layout::{ComputerRecord, SecretRecord},
};
use serde_json::{json, Value};

/// The hosts the GitHub credential may be substituted for: the same three a virtual machine's
/// `SILO_GITHUB` secret names.
pub const GITHUB_HOSTS: [&str; 3] = ["github.com", "api.github.com", "uploads.github.com"];

fn host_entry(host: &str) -> Value {
    if host == "*" {
        json!("any")
    } else if host.contains('*') {
        json!({"wildcard": host})
    } else {
        json!({"exact": host})
    }
}

fn entry(name: &str, source: &str, hosts: &[String]) -> Value {
    json!({
        "env_var": name,
        "source": {"kind": "env", "var": source},
        "value": "",
        "require_tls_identity": true,
        "placeholder": format!("$MSB_{name}"),
        "allowed_hosts": hosts.iter().map(|host| host_entry(host)).collect::<Vec<_>>(),
    })
}

/// The `network` section of an inspected computer: the policy (the gateway enforces the real one;
/// this is what a fork is asked to keep), the secrets assigned (`names` limits it to
/// those the running gateway holds, for `active_config`) and the TLS handling they need.
pub fn network_value(record: &ComputerRecord, names: Option<&[String]>) -> Value {
    let mut entries: Vec<Value> = record
        .secrets
        .iter()
        .filter(|secret| names.is_none_or(|names| names.contains(&secret.name)))
        .map(|secret| {
            entry(
                &secret.name,
                &crate::runtime::secret_source_name(&secret.name),
                &secret.allowed_hosts,
            )
        })
        .collect();
    if record.github {
        let hosts: Vec<String> = GITHUB_HOSTS.map(str::to_owned).to_vec();
        entries.push(entry(GITHUB_SECRET_NAME, GITHUB_SECRET_NAME, &hosts));
    }
    json!({
        "tls": {"enabled": true},
        "policy": {"default_egress": "allow", "default_ingress": "deny", "rules": []},
        "secrets": {"secrets": entries},
    })
}

/// The guest environment that carries the placeholders: `NAME=$MSB_NAME` for each secret the
/// running gateway holds.
pub fn placeholder_env(names: &[String]) -> Vec<(String, String)> {
    names
        .iter()
        .map(|name| (name.clone(), format!("$MSB_{name}")))
        .collect()
}

fn is_github(spec: &SecretSpec) -> bool {
    spec.name == GITHUB_SECRET_NAME
}

/// Whether a `modify` asks for the GitHub secret with the hosts a computer is created with.
fn github_hosts_match(spec: &SecretSpec) -> bool {
    let mut asked = spec.allowed_hosts.clone();
    asked.sort();
    let mut known = GITHUB_HOSTS.map(str::to_owned).to_vec();
    known.sort();
    asked == known
}

/// Records a `modify` in the computer's record. Refuses a GitHub secret for other hosts than
/// GitHub's own.
pub fn apply_to_record(record: &mut ComputerRecord, modify: &Modify) -> Result<(), String> {
    for spec in &modify.put {
        if is_github(spec) {
            if !github_hosts_match(spec) {
                return Err("GitHub access can be used for GitHub hosts only.".into());
            }
            record.github = true;
            continue;
        }
        let secret = SecretRecord {
            name: spec.name.clone(),
            allowed_hosts: spec.allowed_hosts.clone(),
        };
        match record
            .secrets
            .iter_mut()
            .find(|known| known.name == spec.name)
        {
            Some(known) => *known = secret,
            None => record.secrets.push(secret),
        }
    }
    for name in &modify.remove {
        record.secrets.retain(|known| &known.name != name);
    }
    record.secrets.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::containers::capability::make_container;
    use crate::containers::layout::Layout;

    fn record() -> ComputerRecord {
        let home = tempfile::tempdir().unwrap();
        make_container(home.path(), "box");
        Layout::new(home.path())
            .computer("box")
            .read_record()
            .unwrap()
    }

    fn put(name: &str, hosts: &[&str]) -> SecretSpec {
        SecretSpec {
            name: name.into(),
            allowed_hosts: hosts.iter().map(|host| (*host).into()).collect(),
        }
    }

    fn modify(put: Vec<SecretSpec>, remove: &[&str]) -> Modify {
        Modify {
            name: "box".into(),
            put,
            remove: remove.iter().map(|name| (*name).into()).collect(),
            next_start: false,
        }
    }

    #[test]
    fn a_secret_is_recorded_by_name_and_hosts_and_replaced_by_the_same_name() {
        let mut record = record();
        apply_to_record(
            &mut record,
            &modify(
                vec![
                    put("TOKEN", &["api.example.test"]),
                    put("ALPHA", &["*.example.test"]),
                ],
                &[],
            ),
        )
        .unwrap();
        assert_eq!(
            record
                .secrets
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            ["ALPHA", "TOKEN"]
        );
        apply_to_record(
            &mut record,
            &modify(vec![put("TOKEN", &["other.example.test"])], &[]),
        )
        .unwrap();
        assert_eq!(record.secrets.len(), 2);
        assert_eq!(record.secrets[1].allowed_hosts, ["other.example.test"]);
        apply_to_record(&mut record, &modify(Vec::new(), &["ALPHA", "MISSING"])).unwrap();
        assert_eq!(record.secrets.len(), 1);
    }

    #[test]
    fn github_access_is_a_flag_and_only_for_githubs_own_hosts() {
        let mut record = record();
        let error = apply_to_record(
            &mut record,
            &modify(
                vec![put("SILO_GITHUB", &["github.com", "evil.example"])],
                &[],
            ),
        )
        .unwrap_err();
        assert!(error.contains("GitHub hosts only"), "{error}");
        assert!(!record.github);
        apply_to_record(
            &mut record,
            &modify(vec![put("SILO_GITHUB", &GITHUB_HOSTS)], &[]),
        )
        .unwrap();
        assert!(record.github && record.secrets.is_empty());
    }

    #[test]
    fn inspect_reports_the_shape_the_shared_secret_checks_read() {
        let mut record = record();
        record.github = true;
        apply_to_record(
            &mut record,
            &modify(
                vec![put(
                    "TOKEN",
                    &["api.example.test", "*.cdn.example.test", "*"],
                )],
                &[],
            ),
        )
        .unwrap();
        let network = network_value(&record, None);
        assert_eq!(network["tls"]["enabled"], true);
        assert_eq!(network["policy"]["default_egress"], "allow");
        assert_eq!(network["policy"]["rules"], json!([]));
        let entries = network["secrets"]["secrets"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        let token = &entries[0];
        assert_eq!(token["env_var"], "TOKEN");
        assert_eq!(token["placeholder"], "$MSB_TOKEN");
        assert_eq!(token["value"], "");
        assert_eq!(token["require_tls_identity"], true);
        assert_eq!(token["source"]["kind"], "env");
        assert_eq!(
            token["source"]["var"],
            crate::runtime::secret_source_name("TOKEN")
        );
        assert_eq!(
            token["allowed_hosts"],
            json!([{"exact": "api.example.test"}, {"wildcard": "*.cdn.example.test"}, "any"])
        );
        assert_eq!(entries[1]["env_var"], "SILO_GITHUB");
        assert_eq!(entries[1]["placeholder"], "$MSB_SILO_GITHUB");
        // Only what the running gateway holds is active.
        let active = network_value(&record, Some(&["OTHER".to_owned()]));
        assert_eq!(active["secrets"]["secrets"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn the_guest_sees_a_placeholder_never_a_value() {
        assert_eq!(
            placeholder_env(&["TOKEN".into()]),
            [("TOKEN".to_owned(), "$MSB_TOKEN".to_owned())]
        );
    }
}
