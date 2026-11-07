# Forks, sharing, CI runner, containers: build log

Plan: [PLAN.md](PLAN.md).

## Resume

- **Phase:** design questions out to the advisor and Fable; F1 next on `claude/forks-phase-1`.
- **Linux KVM host:** the owner's test machine (10.77.77.2) did not answer on 7 Nov (no route to
  host). Linux measurements run on the GitHub KVM runner (`linux-host.yml`) until it is back; each
  result names the machine it ran on.

## Start (7 Nov 2026)

- Start commit: `cabcfec6` (main).
- Remote hosts: phases 1-6 merged (#75-#80, then #81, #82); phase 6's Mac live checks and an engine
  release with the Linux archives are open. The owner said to treat it as done enough.

## Facts found before planning

- **Forks today** (`engine/src/runtime/checkpoints.rs`, `cli/src/ops.rs`): every Linux fork cold-boots
  from the disks of a checkpoint (`pending_checkpoint_restore.state = "disk"`). With no checkpoint
  named, the fork job first captures a "Fork point" of the source; captured while the source runs,
  that is a full (memory and disk) snapshot whose memory no fork uses. `silo up` saves a warm
  checkpoint; a fork reuses it only while the source is running, its change generation is unchanged
  and the checkpoint is at most 60 s old (`fresh_warm`, `ops.rs:857`). So the warm checkpoint saves
  only the capture. The v1 evidence has no split of fork time beyond the CLI's "Starting: 5s" line.
- **Desktop:** guest image v4+ always starts the desktop at boot (`desktop.rs`
  `default_new_computer_desktops` forces `built_in` and `start_with_computer`); `fork --gui` only
  waits for it. The guest has `silo-desktop autostart false`.
- **Measurement scripts** (`docs/v1/checks/final_*`) are macOS-only (`sysctl`, `footprint`, bundle
  paths). The KVM CI job times nothing.
- **Export/import** (`engine/src/backup.rs`): `exports.*` API, `.silo-backup` archive (magic, manifest,
  per-computer MicroSandbox `snapshot save --with-image` payloads; macOS through
  `macos_archive.rs`). Network policy, secrets and ports are reset; import makes a new stopped
  computer with a new id and a deny-all network policy. No CLI. Not routed over Connections by
  the CLI; the archive path is on the owning device. The guest image is embedded; its pin is in
  `guest-image/image-lock.json`.
- **Credential detection** (`engine/src/changes/ignore.rs` `credential_group`): path patterns on diff
  entries only. A Claude Code login (`~/.claude/.credentials.json`) is not in the lists.
- **Keep of an imported computer:** keep diffs a fork against its fork-time record (`Slot::Base`);
  an imported computer has none, so `silo fold --into` fails today.
- **Engine socket:** 4-byte length-prefixed JSON, `engine.handshake` at protocol 7, same-UID peer check;
  job methods return `jobId`, polled with `operations.job.get`. `guest.exec.start` runs long commands.
  The CLI has `--json` everywhere.
- **actions/scaleset:** v0.5.0 (23 Oct 2026). `listener.Run` with a `Scaler`; `GenerateJitRunnerConfig`
  returns the encoded config the runner takes as `--jitconfig` or `ACTIONS_RUNNER_INPUT_JITCONFIG`;
  `RemoveRunner`; `RunnerSetting.DisableUpdate`. Issue #131 (listener stall) closed 6 Nov 2026; #124,
  #107, #114 open.
- **runscaler** (MIT): readiness on "Listening for Jobs"; capacity broker before the JIT config;
  deregister before delete (`JobStillRunningError` keeps it); orphan reconcile at startup; disk
  guard; `TartHostCoordinator(2)` for macOS.
- **GitHub:** self-hosted runners "should almost never be used for public repositories"
  (docs.github.com/en/actions/reference/security/secure-use); with updates disabled a runner must
  be updated "within 30 days of a new version being made available"
  (docs.github.com/en/actions/reference/runners/self-hosted-runners).

## Decisions

## Status
