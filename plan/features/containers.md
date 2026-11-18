## Decided

- 2026-11-10: containers stay a choice the user can make, for a dev-only Linux machine with nothing sensitive, as the faster option. Never the default, and never decided by the host.
- 2026-11-10, an idea for later: the secrets proxy runs on another device and the container host's traffic is forced through it, so an escape finds nothing.

## Proposed

- Rule: Containers stay the one lighter kind of computer; gVisor and Kubernetes are not offered as a computer's boundary.

## Open

- Can the lighter, kernel-sharing kind be chosen for a single fork, for work you trust, or only for a whole computer?
  - Suggested: Only for a whole computer, because you kept containers for a dev-only Linux machine with nothing sensitive and a fork is a copy of its source.
- To find out: Does youki give real x86 on a Mac?
- To find out: Can youki be used as an image in microsandbox?

## From the old code

The old code had it partly built, about 17,000 lines, and it needs reworking.

Lessons:
- A container shares the host kernel: never the default, and an agent's create of one asks.
- Chrome inside needs --no-sandbox because the hardening blocks nested user namespaces, so a browser compromise reaches the whole computer.
- There is no rootless bound on the writable layer; protect the host's free space instead (refuse below a margin, stop the largest).
- Host prerequisites (subuid, /dev/net/tun, AppArmor userns, cgroup delegation) must be checked and named, never set by Silo.
- A running container's checkpoint must freeze its processes; one device-wide lock covers every freeze and thaw.
- Unix socket paths over 107 bytes fail (SUN_LEN); keep state paths short.
- Only live KVM runs found the container bugs (ACLs breaking unpack, socket path over 107 bytes, XDG_RUNTIME_DIR).
- An escape as the agent lands as the engine's own user, which holds the host's secrets.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/src-tauri/engine/src/containers`
- `docs/forks-ci/CONTAINERS-PLAN.md`
- `docs/forks-ci/evidence/c1-containers/README.md`
- `docs/forks-ci/evidence/ct1-gateway/README.md`
- `app/SiloUI/src-tauri/engine/src/containers/lifecycle.rs`
- `app/SiloUI/src-tauri/engine/src/containers/mod.rs`
- `app/SiloUI/container-shim/src/main.rs`
- `docs/forks-ci/BUILD_LOG.md#L539`
- `app/SiloUI/src-tauri/engine/src/containers/layers.rs`
- `app/SiloUI/src-tauri/engine/src/containers/snapshots.rs`
- `app/SiloUI/src-tauri/engine/src/containers/archive.rs`
- `app/SiloUI/container-shim/src/merge.rs`

Calls: `computers.create`, `silo create --container`, `silo host init`, `checkpoints.create`, `checkpoints.fork`, `exports.create`.
