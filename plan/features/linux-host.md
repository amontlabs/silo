## From the old code

The old code had it partly built, about 5,000 lines, and it needs reworking.

Lessons:
- An unsigned Linux engine directory must be owned and writable only by its account or root, all the way up from /, or anyone can swap the binaries.
- Use KillMode=process: VM processes are detached on purpose so an update keeps them running.
- Refuse to run as root or nobody; use a dedicated account in the kvm group.
- sshd StrictModes ignores authorized_keys when home or ~/.ssh is group-writable; report the exact chmod.
- Never run anything as root or turn lingering on; print the advice instead.
- Existing .ssh modes survive key install unless repaired (review GB-01).
- A default ACL made ssh-keygen write a world-readable key on the runner, so the engine sets 0600 itself.
- Linux app builds and packages were discontinued; only the engine runs on Linux hosts.

Where it lived, at the tag legacy-2026-11-16:
- `docs/hosts/BUILD_LOG.md`
- `app/SiloUI/src-tauri/silod/src/service_linux.rs`
- `app/SiloUI/src-tauri/silod/src/install_trust.rs`
- `app/SiloUI/scripts/linux-host-e2e.sh`
- `.github/workflows/linux-host.yml`
- `app/SiloUI/cli/src/hosts.rs`
- `app/SiloUI/cli/tests/hosts.rs`
- `app/SiloUI/scripts/hosts-e2e.sh`
- `docs/SiloUI-CLI.md#hosting-computers-for-other-devices`
- `docs/THREAT-MODEL.md#L812`
- `docs/hosts/BUILD_LOG.md#L60`
- `docs/ROADMAP.md#L375`

Calls: `silo host init`, `silo host status`, `silod --service register`, `silo host revoke`, `connections.keys.authorize`, `silod --service`.
