//! Per-computer network gateways for ready macOS guests.

mod endpoints;

pub(super) use endpoints::{endpoint, mark_unavailable, publish, remove};

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod supervisor;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod bootstrap;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
// These narrow hooks are consumed by the parallel provisioning and launch-migration owners.
#[allow(unused_imports)]
pub(super) use bootstrap::{
    migrate_legacy_guest_bootstrap, require_guest_bootstrap, stage_guest_bootstrap,
    sync_guest_placeholders, verify_guest_ready,
};

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(super) use supervisor::{
    apply_github_profile, apply_github_profile_state, apply_secrets, generate_ca,
    github_profile_is_cached, guest_secret_names, issue_trust_probe, prepare, revoke_secret,
    scoped_cached_tokens, with_policy_lock, PreparedGateway,
};
