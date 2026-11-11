# MicroSandbox 0.7.6 → 0.7.15 upgrade

Silo branch: `feat/msb-0.7.15` (from `direction` at `be7d35cae`), checkout `~/code/projects/silo-msb-0715`.
Upstream clone: `~/code/projects/msb-upstream/main` (tags `v0.7.15`, 0.7.6 = `09df3d4b9`).

## Patch decisions (from code reading, 2026-11-11)

Drop, already in 0.7.15: remove-created (#1724), restore-starting-control (#1741),
relay-closed-local-arena (#1796), metrics-sampler-blocking (#1807), reordered-image-metadata
(#1744), portable-image-cache (#1712).

Shrink:
- sftp-user: keep only `USER`/`LOGNAME` in the SSH exec env; upstream #1713 runs SFTP as the
  session user, so drop our `/usr/lib/openssh/sftp-server` branch unless it is still needed.
- checkpoint-fs-state: per-device limit is upstream (#1751, `fs_state_limit`); keep only the
  aggregate budget and the device inventory check.
- retired-layer-cleanup: replace with our upstream PR #2044 (`~/code/projects/msb-upstream/compact-retired-root-layers`).
- reclaim-capture-dirs: keep; compare with `~/code/projects/msb-upstream/remove-consumed-full-capture`.

Keep and port: silo-network, restore-policy, create-stopped, adopt-owned-disk,
log-retention-desktop-start, restore-root-capacity (reproduce first), live-public-ports,
secret-values-stdin, import-stage-id, runtime-instance-id, macos-proxy, macos-network-fixes,
rename-stopped, secret-usage-events, restore-net-strict (added on `direction`).

## Porting chains (parallel)

Each chain is a branch `silo/0715-<chain>` from `v0.7.15` in its own worktree
`~/code/projects/msb-upstream/silo-0715-<chain>`, one commit per patch named `silo: <patch>`.
The integration step stacks the chains and regenerates `app/SiloUI/patches/*-0.7.15.patch`.

| Chain | Patches, in order |
| --- | --- |
| A cli-secrets | silo-network, restore-policy, secret-values-stdin, sftp-user (shrunk), runtime-instance-id, restore-net-strict, secret-usage-events |
| B lifecycle | create-stopped, adopt-owned-disk, restore-root-capacity, rename-stopped |
| C network | live-public-ports, macos-proxy, macos-network-fixes |
| D snapshot | import-stage-id, checkpoint-fs-state (shrunk), reclaim-capture-dirs, retired-layer-cleanup → #2044, log-retention-desktop-start |
| E migration | 0.7.12 storage migration: test a 0.7.6 home with 0.7.15, design and build Silo's upgrade step |

## Silo-side changes owed after the stack builds

- `runtime-inputs.json`: version, `sourceCommit`, archive hash, asset hashes, `libkrunfwCommit`, patch list.
- `engine/Cargo.toml` microsandbox rev; `macos-gateway/Cargo.toml` `msb_krun =0.1.45`, `msb_smoltcp` fork; gateway API (`SmoltcpNetwork::start` returns `Result`, `http_deny_body` replaced).
- Behaviour: JSON 403 policy denials (#1825), start fails on a taken published port (#2030),
  restore refuses recorded host mounts (#1735) for legacy `-v` mounts, keep `--no-stdin`.
- Guest image rebuild (new agentd).

## Log

- 2026-11-11: plan written; chains A–E started.
- 2026-11-11: chain D done on `silo/0715-snapshot` (6 commits, tests pass). Found: v0.7.15's
  lockfile needs rustc 1.95+ (`lock_api`, `parking_lot`), so `runtime-inputs.json` `toolchain`
  1.94.0 must move; the v0.7.15 agentd download URL returned 404 (builds used
  `MSB_EMBED_ARTIFACTS_DIR=~/code/projects/msb-upstream/check-artifacts`, check-only copies).
  Capture dirs moved to `<sandbox>/checkpoints` in 0.7.15. `grow-retire-old-root-chain` included.
- 2026-11-11: chain C done on `silo/0715-network` (3 commits; network crate 760 tests pass).
  Integrator owes: silo-network's `legacy_changes`/`subscribe()` hunks in `secrets/handle.rs`
  (left out of C); gateway must handle `start()` -> `Result` and `start_and_wait_for_ports(handle)`
  (no timeout, new `PortStartupError`). Live ports now share the global inbound budget.
