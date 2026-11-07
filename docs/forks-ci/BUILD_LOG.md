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

Design questions: [DESIGN-QUESTIONS.md](DESIGN-QUESTIONS.md). Advised by Fable 5.1 and the advisor
(gemini-4-argon), 7 Nov 2026.

- **Q1, fork source (decided by the owner, 7 Nov): a fork starts from the source as it is now.** F1 measured a
  fork from `silo up`'s warm checkpoint at 8.13 s against 9.28 s for a fork that captures first (Mac,
  medians, overlapping ranges), so the warm checkpoint saves about 1 s. Forks capture the source's disk only
  (no memory), and `fresh_warm`, the 60 s limit and the generation counter are removed. Fable's design; the
  advisor preferred the newest saved checkpoint.
- **Q2, desktop (decided by the owner, 7 Nov):** a setting per computer, on by default. A recipe can say
  `desktop: false`; the computer and its forks then start no desktop, and `--gui` and computer use say the
  computer has no desktop. The desktop can be turned on later for that computer.
- **Q3, keeping an imported computer:** the export carries the computer's fork-time record when it
  has one; import stores it as the imported computer's fork-time record, so `silo fold --into main` is
  an ordinary three-way keep. A computer without one (a main) imports as its own computer and cannot be
  kept into another. Both advisors.
- **Q4, guest image:** exports stop embedding the guest image (no `--with-image`); import already
  refuses an archive whose image digest differs from this Silo's pinned image (`backup.rs` import
  checks), so the embedded copy is never used. The receiver needs the same pinned image, prepared as for
  a new computer. Fable; the advisor's "pull any digest from the registry" adds an offline failure mode
  for a case import refuses anyway.
- **Q5, export from another machine:** export on the owning machine into a Silo-owned temporary file,
  copy it over the existing Connections SSH link, then remove the remote file; leftovers are named so a
  later run removes them. Both advisors.
- **Q6, runner helper:** shell out to `silo --json` (it already handles the protocol, job polling,
  channels and `--device` routing); one scale set per recipe label; the runner is named after its
  computer (GitHub shows the runner name on every job page, so no hook is needed); the JIT config goes in
  through the environment (`ACTIONS_RUNNER_INPUT_JITCONFIG`), never the command line; on success
  deregister then delete, on failure deregister then stop and keep, with a cap on how many failed
  computers are kept and for how long; macOS capacity counts every running macOS VM on the Mac. Both
  advisors (the advisor's job-started hook is dropped for the runner name).
- **Login warning:** add Claude Code (`~/.claude/.credentials.json`) and other agent logins to the
  credential patterns; the export check scans the computer's files and its fork-time record.

## Status

### 7 Nov 2026: phase F1 measured (before)

- **Script:** `docs/forks-ci/checks/measure_forks.sh` (Mac and Linux), run on the Mac by hand and on the
  KVM runner by the `Fork measurements on KVM` job of `linux-host.yml`.
- **Mac** (M4 Max, Dev engine at 620a4683; [evidence](evidence/f1-before/mac-m4max-dev/README.md)): cold
  fork 9.28 s, warm single fork 8.13 s, `-n 4` 16.01 s, RAM per fork 498 MiB (footprint), `-n 10` 36.76 s
  with 4997 MiB in total. macOS fork not measured: creating a macOS source failed ("did not accept SSH
  logins in time").
- **Linux KVM** (GitHub runner, 4-core Xeon, 15.6 GiB, nested KVM; [evidence](evidence/f1-before/linux-kvm-gh/README.md)):
  cold fork 11.03 s, warm single fork 10.14 s, `-n 4` 36.20 s, RAM per fork 1027 MiB (PSS); `-n 10` not
  measured (not enough memory on the runner).
- **Result:** the warm checkpoint saves about 1 s on one fork on both hosts, which decided Q1.
- **Reviews:** Fable (method, numbers traced to raw files, Linux portability, the CI job's triggers and
  permissions): no blocking finding; the two medium findings (median of the RAM gate, the script commit
  in the evidence) fixed in `d8ca5985`.

### 7 Nov 2026: phase F2 built and measured (after)

Both owner decisions:
  - **Fork source.** MicroSandbox 0.7.6 already takes a consistent disk-only snapshot of a running microVM, so no patch was needed (nothing for `docs/v1/upstream`): `snapshot create --from-sandbox` without `--full` sends `DiskCheckpointCreate`, and the runtime's `capture_disk` flushes the guest filesystems (`--guest-flush required`), pauses the VM, seals the disks, resumes (about 0.1 s paused). The engine's fork point (`capture_fork_point`, `CaptureDepth::DisksOnly`) now uses that on Linux; manual, automatic, before-keep and step checkpoints still save memory. macOS fork points were already disk-only (`capture_held` with `PolicyKind::ForkPoint`, a guest `sync` first). `silo fork -n N` takes one fork-point capture through `checkpoints.create` with the existing `fork-point` kind (disks only on both platforms; it was refused before) and the N forks share it.
  - **Removed.** `fresh_warm`, `warm_max_age` (a `silo.yaml` that still names it now fails to load, as for any unknown key), the warm checkpoint of `silo up` (`up` deletes one left by an earlier version), the `generation` fields of `computers.list` and of checkpoints, the in-flight and warm tracking of `engine/src/generation.rs`, and the SSH-session tracking that fed it. `generation.rs` stays, reduced to the per-computer change counter that SSH access still uses to skip inspecting a stopped computer until a lifecycle change. The `warm` checkpoint kind stays readable (saved ones still load, can be deleted) and `checkpoints.create` refuses it.
  - **Desktop setting.** The per-computer setting is the existing `desktop.startWithComputer`. New v4+ computers keep an explicit `false` (`default_new_computer_desktops`), and `change_computer` no longer forces a built-in desktop back on. A recipe can say `desktop: false` (Linux only; not with `services:`, which start with the desktop session, and refused for a macOS recipe); it is part of the build hash only when false. Forks inherit it through the cloned configuration and the guest's own `silo-desktop autostart`. `silo desktop on|off <computer>` (running Linux computers) uses the existing `computers.configuration.change` and `desktop.action`. `silo fork --gui`, the MCP computer-use tools, `silo status` and the app's desktop viewer say `<name> has no desktop; turn it on with ...`; the app's computer settings have a Desktop switch for built-in computers. macOS: its display is the computer, so there is no setting and computer use stays.
  - **Found by the live run:** a computer with its desktop off held its operation lane about 5 minutes after each start (the guest's computer-use boot waits 300 s for a session), which made the first fork of it take 288 s (observed during development, not in the evidence's `raw/`). Fixed in the engine without a guest change: no computer-use apply at boot or creation while the desktop is off; turning it on applies it.
  - **Tests.** `cargo test -p silo-engine --lib` 3106 passed (the 8 `host_push::protocol_tests` need the prepared Git runtime and were skipped), `-p silod -p silo-api` and `-p silo-cli` pass, Rust format check, `npm run typecheck`, `lint` and `npm test` (2856) pass.
  - **Evidence:** [f2-after/mac-m4max-dev](evidence/f2-after/mac-m4max-dev/README.md), Dev engine from a release-profile bundle. A fork of a running idle `main`: 8.47 s against 9.28 s; with a desktop 497 MiB, without 211 MiB and 7.76 s against 8.61 s (a single fork of a recipe computer: 494 MiB with a desktop, the TOTAL of `raw/plain-fp.txt`, as for 211 MiB without); the rows that used the warm checkpoint are 0.5 to 1.8 s slower (`-n 4`: 17.78 s against 16.01 s).
  - **Measured builds.** The engine builds measured (`eeeb49b6`, `b696d9e3`) predate the squashed PR commit; the numbers were not re-measured on it.
  - **Review fixes.** "Has no desktop" applies only to a built-in desktop (guest image v4+): `Computer::desktop_on` is `None` for an older Linux computer, `silo desktop` refuses one, and the app viewer's "has no desktop" needs the status's `computerUse` (reported only for a built-in desktop); an older computer keeps "Desktop is stopped". The settings switch and the viewer say the same words, and neither promises computer use while the setting is off. `checkpoints.create` of a fork point no longer queues the no-op trim job. A `silo.yaml` that names `warm_max_age` fails with a message naming the key and telling the owner to delete the line (tested).
  - **Open:** on Linux a `silo fork -n N` shares a manual-kind "silo-fork-<id>" checkpoint that stays on the source. It cannot be removed after the forks exist (they use the snapshot member, so deletion is refused as in use), and a fork-point kind removed with its last fork would need the macOS `release_fork_point` machinery rebuilt on Linux's snapshot lineage; not done here.
  - **Not done:** a macOS fork timing (no macOS computer on Dev), the Linux KVM host.
  - **Reviews:** Fable LGTM, Opus LGTM.
