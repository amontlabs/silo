# Moving existing computers to MicroSandbox 0.7.15 storage

Status: facts verified 2026-11-11 against upstream `v0.7.15` and a throwaway 0.7.6 home; the engine step is built and unit-tested but not yet called from the launch driver.

## What upstream does (v0.7.15)

- 0.7.12 moved disk records and checkpoints out of guest-accessible folders ("protected state"; marker `<sandbox>/state-layout.json`, phase `ready`; the old `runtime/root-disk.json` becomes an upgrade guard so 0.7.6 refuses the sandbox). Source: `crates/runtime/lib/checkpoint/layout.rs`, `sdk/rust/lib/runtime/protected_state.rs`.
- **Every** sandbox without a `ready` marker is refused at `msb start` on macOS, however simple (`protected_state.rs:248` `require_maintenance`, called from `prepare_storage` before any planning). It is not limited to snapshots, forks or owned disks. On Linux and Windows supported storage migrates silently at start.
- The macOS "maintenance prompt" is the caller's attestation that every legacy VM in every MicroSandbox home has stopped and nothing old will write the disks until repair ends. Upstream will not infer this (old full snapshots can leave writable aliases in another home's export, and macOS has no per-file writer lease). It is `StorageRecoveryOptions::legacy_writers_stopped` (`sdk/rust/lib/sandbox/storage_recovery.rs:46`).
- Only the interactive `msb self repair` sets that field (`repair/interactive.rs:207,435`). The scripted path (`--format json`, or any non-terminal stdio) on macOS always returns exit 2 with "requires interactive maintenance confirmation" (`repair.rs:150-171`); the former `--legacy-writers-stopped` and `--accept-selected-view` options were removed on purpose (`repair.rs` test `removed_selection_and_confirmation_flags_are_rejected`). There is no `--yes` or environment variable. The Rust SDK call `Sandbox::recover_storage(name, StorageRecoveryOptions { legacy_writers_stopped: true, .. })` is public.
- Scripted repair also refuses when a VM must stop (exit 2, "Run msb stop X, then repair"), when status is `recovery_required` (ambiguous or damaged legacy metadata; needs an interactive view selection and the "may omit later writes" acknowledgement), `incomplete_creation`, or `unavailable`.
- What repair does to files (`migration/` engine, `layout.rs`): copies the validated current view of each disk (root, each owned disk, snapshot chain) into `<sandbox>/.storage-migration/<disk>/layer-NNN.*` (cloned, so cheap on APFS), writes host-owned `<sandbox>/root-disk.json`, `state-layout.json`, `guest-state/`, and replaces `runtime/root-disk.json` with the upgrade guard. The original `upper.ext4`, `upper-*.qcow2` and `owned-volumes/*` are retained untouched. It uses current data, including writes after the last snapshot. The sandbox is left stopped. Rerunnable after interruption if inputs are unchanged.
- The catalog is upgraded in place on first open by 0.7.15 (migrations through `m20261030_000001_protected_sandbox_state`), and `doctor` is read-only. A 0.7.6 `msb` then refuses the home ("database schema is newer"), so the move is one-way.
- Running matters: a running legacy VM blocks repair for itself and every related sandbox (snapshot/fork lineage); `exec` and `stop` still work in 0.7.15. Stopped is required.

## Empirical results (throwaway home, 0.7.6 patched build -> 0.7.15 CLI built from the `v0.7.15` tag)

Home `app/SiloUI/src-tauri/target/verification/msb-migration/` (`e2e-*` only). Created with 0.7.6: `e2e-plain` (started once, stopped), `e2e-disk` (owned 512M disk, disk snapshot, later writes), `e2e-child` (snapshot restore), `e2e-fork` (`msb fork` of a running VM).

| Check | Result |
| --- | --- |
| `doctor --sandbox X --format json`, all four | `maintenance_required` (inspection `eligible`, 1-2 candidate views), none `recovery_required` |
| `msb start e2e-plain` | refused: "macOS legacy storage requires explicit maintenance recovery" |
| `self repair --format json` / non-tty | exit 2, same maintenance text, nothing changed |
| interactive repair (pty, `y` to both prompts) | all four `repaired; ready to start`; originals retained, `.storage-migration` 4 MB (cloned) |
| start after repair, read data | `plain`; `e2e-disk` kept `disk1` and the post-snapshot `disk2-after-snap`; child kept `disk1` + `child` |
| old snapshot after repair | listed as `canonical`; `snapshot restore e2e-disk:e2e-snap1` works and has the snapshot-time data; new snapshot and `msb fork` of a migrated VM work |
| legacy VM running during repair | doctor `running: true`; scripted: "Run msb stop"; interactive asks to stop it, then repairs |
| 0.7.6 binary on the migrated home | refuses (database schema newer): one-way confirmed |
| home copied to another path before repair | `recovery_required` ("legacy root journal disagrees with the configured disk role"): journals hold absolute paths, so repair must run in the original location |

Not tested: grown or compacted disks, cross-home aliases, `needs investigation` outcomes, Linux guests with Silo's real image (the guest was alpine with the stock agent), a 0.7.15 agent in the guest.

## Design

Runs once per storage folder at the first launch with the 0.7.15 runtime, inside the existing migration gate (`runtime_migration`), after any pending checkpoint-format conversion and before computers can be used. In place: the repair cannot run on a relocated copy.

1. **Gate.** No runtime command other than this step runs; the Silo engine is the only user of the private home (`runtime_home_alias`), so the macOS attestation is true by construction: all local computers are Silo's, stopped below, and no previous Silo build can open the upgraded catalog.
2. **Inspect** every computer with `msb doctor --sandbox X --format json`. `ready` stays as is. `eligible`, `maintenance_required`, `in_progress` are repairable. `recovery_required` and `unavailable` are `NeedsAttention` and never touched.
3. **Stop** any computer still running (a crashed or force-quit session) with `msb stop`, then check it stopped. One that does not stop is `NeedsAttention`.
4. **Back up** once, after the stops and before the first repair: a clone of the storage folder (`copy_runtime_tree`, cloned files) to `<app_data>/runtime-backup-before-protected-storage`, written as `.partial` and renamed, so a crash never leaves a trusted partial. A failed backup ends the step before any repair. Rollback is by hand: with Silo closed, swap the two folders; the clone names the original paths, so it only works in the original place. Retention and deletion UI is the existing pre-upgrade backup's job (14 days, user can delete); extend `pre_upgrade_backup.rs` to know this folder (open item).
5. **Repair** each repairable computer: `msb self repair --sandbox X --format json --legacy-writers-stopped` (macOS only; patch below), then `doctor` must report `ready`. A refusal, failure or unverified result is `NeedsAttention` for that computer only; the others continue. Repair is rerunnable, so a crash resumes at the next launch. Originals stay on disk.
6. **Record** `protected-storage.json` in the storage folder (atomic). Only a report with no `NeedsAttention` ends the step; otherwise it runs again next launch (cheap: `doctor` only for the ready ones) and a Silo update that teaches the runtime more can finish them.

User-facing states: progress "Upgrading computer storage (n of m)" through the existing `migration.changed` state. A computer in `NeedsAttention` shows as "Needs attention: <message>", cannot start, is never deleted or reset, and keeps its files; the rest of Silo works. Message text is the plain one in the module (no msb output). Owner wording for the error that cannot be self-served: "Its files were kept untouched. Report this problem before using the computer." No terminal command is ever shown.

Why not the alternatives: a helper linking the Rust SDK (`recover_storage`) needs the whole `microsandbox` crate and a second process contract in the engine, which today only drives the `msb` binary; restoring the interactive prompt through a pty would make Silo answer a safety prompt by keystroke; deleting and recreating computers loses data.

## Patch needed in the bundled msb (not written here)

File `crates/cli/lib/commands/self_cmd/repair.rs`, patch name `repair-supervisor-maintenance`:

- `RepairArgs`: add `#[arg(long, hide = true, requires = "format", requires = "repair_target")] pub legacy_writers_stopped: bool` (valid only with `--format json` and an explicit sandbox).
- `run_scripted`: replace the unconditional `if cfg!(target_os = "macos") { return blocked(...) }` with `if cfg!(target_os = "macos") && !args.legacy_writers_stopped { return blocked(...) }`, and call `Sandbox::recover_storage(&name, StorageRecoveryOptions { legacy_writers_stopped: args.legacy_writers_stopped, ..Default::default() })`. `args` is partially moved by `args.name` today, so read the flag first.
- Keep the other scripted refusals (running VM, `recovery_required`, `incomplete_creation`, `unavailable`). Update the test `removed_selection_and_confirmation_flags_are_rejected` so only `--select` and `--accept-selected-view` stay rejected, and add one test that the flag is refused without `--format json`.
- The flag is the same attestation upstream asks the terminal user for; Silo's engine passes it only after step 3 and only inside the gate. It should be offered upstream as the supervising-app hook once it has a failing-first test.

## Open items

- Call `protected_storage::upgrade` from the launch driver (new `Controller` stage after conversion), build `Options` with `clone_backup`, pass `confirm_maintenance: cfg!(target_os = "macos")`, publish progress, write the report, and expose `NeedsAttention` computers in the computer list.
- Teach `pre_upgrade_backup.rs` the new backup folder.
- Real-image check once the 0.7.15 guest image is built: migrate a copy of a real 0.7.6 home (never `~/.silo`), start every computer, compare data.
- Decide what to offer for `recovery_required` (it needs an owner choice between views; today Silo keeps the files and reports).
