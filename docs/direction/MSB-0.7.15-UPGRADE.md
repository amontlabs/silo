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
- 2026-11-11: chain A done on `silo/0715-cli-secrets` (7 commits; network 754, CLI restore, ssh
  tests pass). sftp-user keeps the `/usr/lib/openssh/sftp-server` branch for non-root sessions:
  #1713 runs only open/mkdir/symlink as the session user, while rename/remove/chmod/readdir still
  run as root. New probe flag `--silo-restore-strict-protocol`. `should_bypass` means a host with
  no matching secret gets a closed connection, not the #1825 JSON deny page (same as 0.7.6).
- 2026-11-11: chain E done: every 0.7.6 sandbox is refused on macOS until `msb self repair`;
  design and engine step in [MSB-0.7.15-MIGRATION.md](MSB-0.7.15-MIGRATION.md). Repair must run in
  place (absolute paths). Needs a hidden `--legacy-writers-stopped` msb option (branch
  `silo/0715-repair`) and wiring (branch `feat/msb-0715-migration-wiring`).
- 2026-11-11: A, C, D stacked on `silo/0715-integration` (16 commits; one conflict in
  `secrets/handle.rs` resolved to the 0.7.6 stack's final shape). Gateway, toolchain, engine pins
  and behaviour changes on `feat/msb-0715-gateway`.
- 2026-11-11: chain B done; the msb repair confirmation patch added. Full stack on
  `silo/0715-integration` (21 commits): fmt clean, 4,000+ unit tests across 7 crates pass.
- 2026-11-11: **base changed to v0.7.14.** Upstream's "Release v0.7.15" job failed, so v0.7.15 has
  no release assets (agentd, libkrunfw). v0.7.14 is the latest published release; agentd source is
  identical between the two tags, libkrunfw differs (223f1169 vs 83c74b91). The stack now sits on
  `silo/0714-integration` = v0.7.14 + upstream #2032 (file-sharing speed-up, carried as a patch) +
  our 21 commits, with no conflicts. Move to 0.7.15 or later once a release is published.
- 2026-11-11: gateway/toolchain/engine slice done on `feat/msb-0715-gateway`. Gateway builds and
  its 36 tests pass against the integration tree (`msb_krun =0.1.45`; `start_and_wait_for_ports`
  without a timeout; the error code is now `port_bind_failed`; the `msb_smoltcp` fork arrives
  through the network crate, so the gateway needs no direct pin; `http_deny_body` was never used
  and JSON denials are now set explicitly). Gateway `Cargo.lock` regenerated. Rust 1.95.0 is the
  lowest stable that builds v0.7.15 and the gateway; `runtime-inputs.json` `toolchain` and
  `rust-toolchain.toml` (a test keeps them equal) moved to it. Engine compiles against v0.7.15
  (`f9ae8e23`) with no API drift. #1825: nothing in Silo expected dropped connections. #2030: Silo
  never publishes ports through `msb start/fork/restore` (live `port_add` only, already mapped to
  "port in use"). #1735: disk restores now add `--allow-missing-resources` unless legacy `-v`
  mounts are passed; the container shim accepts the flag. #1688/#2030 stdin: Silo never uses
  `msb run`; every exec through `run_msb` keeps `--no-stdin`.
- 2026-11-11: base changed to v0.7.14 (v0.7.15 has no release assets) plus our patches and #2032 on
  `silo/0714-integration`. Engine pin is now v0.7.14 (`60fc5071b34e6cc2671c84c8dc07709795d45d27`);
  gateway lock regenerated and its 36 tests pass against that tree on 1.95.0. The Rust toolchain pin
  (1.95.0 in the entries above) is to be reset to 1.94.0 if v0.7.14 builds on it.
- 2026-11-11: merged gateway/pins/behaviour (`feat/msb-0715-gateway`), storage wiring and its
  review fixes (`feat/msb-0715-migration-wiring`). Patches regenerated from
  `silo/0714-integration` (clippy `-D warnings` clean on 1.95.0 except one upstream test).
  Toolchain 1.95.0 everywhere. Reviews (Sonnet; Codex was out of quota): Silo side 7 findings,
  6 fixed (`2162744c9`), the 7th (restore drops recorded mounts only for computers that have none)
  judged intended; patch stack: no security issue, one lint fix. Full suites: cargo workspace 5093
  passed, vitest 3122 passed, test:release 138 passed / 17 skipped, typecheck, lint, fmt clean.
  Known flaky under load: two `audit::...trimmed` tests, `dependency_hash_fifo_is_rejected_without_waiting_for_a_writer`.
  Next: live check in a private home (never the shared Dev home: the storage upgrade is one-way
  and the direction build still runs 0.7.6 on Dev).
