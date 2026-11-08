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
### 8 Nov 2026: phase C1 measured (crun containers against the microVM)

- **Question and scope.** Is an opt-in container computer on Linux hosts (crun, rootless, all capabilities
  dropped, no-new-privs, a seccomp deny-list, Landlock from a small init shim, its own network namespace with a
  veth pair and default-deny nftables to a host proxy, an overlay root over the guest image, cgroup v2
  limits) worth building? Measurements only; nothing is built into Silo. Scripts in
  `docs/forks-ci/containers/`, workflow `.github/workflows/containers-c1.yml` (only for changes there and by
  hand), results in [evidence/c1-containers](evidence/c1-containers/README.md).
- **Where.** The GitHub ubuntu-24.04 runner (a cloud VM: the microVM runs on nested KVM, the containers run on
  the runner's kernel); the container and microVM measurements ran in the same job on the same CPU (EPYC
  9V74, run 42429983339). The owner's Linux machine was offline. Landlock ABI 8 on the kernel (7.0.0-azure).
- **Numbers** (N=3, medians), fair pairs first: one desktop 315 MiB in a container (763 MiB with Chrome)
  against 840 MiB for a microVM; about 2.3 s of desktop start steps against 5.3 s; ten desktops about 2 GiB
  against about 10 GiB (estimated as one fork's 1032 MiB x 10, not measured: ten microVMs did not fit the
  runner). Ten desktops "serving after 28.4 s" includes 15 s of fixed sleeps; the last Selkies answered about
  12 s after the start (read from the run log). Bare shell, against a microVM that boots a desktop: 0.09 s and
  4 MiB; fork 0.20 s against 13.10 s; four microVM forks 32.09 s. The workload rows (apt, unpack, cargo,
  make) are not like for like (the microVM ran in a fork of main from the demo recipe with build tools
  pre-installed, the container from the bare image) and are not a speed result. Through the proxy, apt's
  download was slower; tinyproxy against Squid was 3-5 times, but no control ran the download through Squid
  from the host without a container, Squid's own spread (23-75 s) is unexplained, and the single-file transfer
  was 39 MB.
- **Baseline held.** Capabilities, seccomp, Landlock, the network policy and the masked paths were checked from
  inside; the desktop (Xvfb, PulseAudio, Xfce, Selkies, Cua Driver's MCP server) runs; Chrome needs
  `--no-sandbox` because the baseline blocks the user namespace its sandbox uses.
- **Costs found.** A root step for the veth and nftables; Ubuntu 24.04's
  `apparmor_restrict_unprivileged_userns` has to be 0; cgroup delegation; a fork's manifest diff must compare
  upper trees both ways (a deleted upper-only file leaves no whiteout; whiteouts are 0/0 devices).
- **Conclusion.** Worth building as a clearly labelled, never-default option for a machine the owner treats
  as disposable: about 2x faster start and 5x lighter on the fair pairs. Costs: shared host kernel, Chrome
  only with `--no-sandbox`, a root helper per container for its network, Ubuntu's AppArmor
  unprivileged-userns guard relaxed host-wide, a two-way diff of upper layers, secrets, tokens and evals
  untested. Not probed: `clone`/`setns` with `CLONE_NEWUSER` (only `unshare`), non-TCP traffic. Not run:
  Silo's secrets proxy, the GitHub token boundary, the guest client, the isolation evals, a Cua Driver
  screenshot, a Selkies client session, bare metal. A PR with the `containers-microvm` label re-runs the long
  microVM job on every push. The decision is the owner's.
### R1: CI runner on the Mac (branch `claude/runner-phase-1`)

- **Built:** `silo-action/` and every reference removed; `silo-runner` (Go, `app/SiloUI/runner`, on
  `actions/scaleset` v0.5.0); recipes `silo-linux` and `silo-macos` (runner 2.338.2, SHA-256 from the
  release digests, linux-arm64 and osx-arm64); `silo exec --stream [--stdin]` in the CLI; example
  workflow; Go CI job; [CI runner](../SiloUI-CI-RUNNER.md); threat-model section; changeset.
- **Long-lived command with a secret, without argv:** `silo exec` was bounded to 180 s and had no input.
  The API's `guest.exec.start` already takes `stdin` and has no time limit, so the CLI gained
  `--stream` (uses `guest.exec.start`, output as it arrives, no step recorded) and `--stdin` (reads the
  CLI's stdin to its end, at most 1 MiB, into the request). Not an env flag: the engine passes
  a guest `env` value as `--env K=V` in the host's `msb exec` arguments (Linux computers) or in the ssh
  command line (macOS computers), where `ps` shows it; stdin is not in either. The recipes' `silo-run-jit` script reads the line into
  `ACTIONS_RUNNER_INPUT_JITCONFIG` and execs `run.sh`.
- **Decisions beyond Q6:** a computer is deleted after `succeeded`, `canceled` and `skipped`, kept
  after anything else; ready means the runner printed "Listening for Jobs"; after `startAttempts`
  failures the helper backs off up to 16 minutes; the recipe is refreshed when a new runner release is
  `afterDays` (3) old, with the digest from the release API; capacity counts `silo list --all`, so a
  macOS recipe build uses a slot.
- **Lifecycle rewritten to [R1-SIMPLER-DESIGN.md](R1-SIMPLER-DESIGN.md) (7 Nov 2026).** Two review
  rounds kept finding ownership bugs in the per-runner flags, goroutines and the separate startup
  reconcile, so the scaler's finish/abandon/settle/retry/retention/adopt/housekeeping paths and
  `reconcile.go` are gone. One event-loop goroutine (`fleet.go`) is the only writer of the records and
  the ledger; `plan()` runs after every event and every 30 s tick and starts the next step of each
  record; the ledger (`state.go`) holds the phases `forking | live | finishing | kept`, written by name
  before the fork and by id after, and fsynced with its directory. Every engine call after the fork goes
  by id (`silo drop` does not stop a running computer and the engine refuses to delete one, so
  `CLI.Drop` stops first); a computer is gone only when a fresh successful `silo list` lacks its id;
  capacity (`capacity.go`) is a pure function of the latest list plus computers asked for that it does
  not show running, with the memory read from the recipe file (the scale set's `memoryGiB` key is
  gone). A job in flight across a restart is adopted as busy and kept with result `unknown`. Non-test Go
  went from 2588 to 2997 lines: the loop needs its event and task types, the list confirmation and the
  probe of an adopted runner, which the old code spread over flags.
- **Review round 3 fixes (7 Nov 2026).** (1) A live record whose runner had a job (its request id is
  recorded on `JobStarted`, or a job result is stored) is adopted as busy at load and probed; if
  GitHub no longer has the runner it ends `unknown` and is kept, instead of going through the orphan
  path and being deleted. Only a live record with no job evidence is finished as an orphan, and that
  now happens at load, so the `subRestored` sub-state is gone. (2) `JobStillRunning` clears only the
  helper's own reasons; a stored job result survives, and the probe finishes with it (a success is
  deleted, a failure stays `failed`) and uses `unknown` only when there is none. (3) The capacity
  demands are every forking or live record the latest list does not show running. (4) A recipe
  rebuild reserves its computer (`Fleet.Reserve`, replacing `CheckCapacity`) until it ends; while the list
  also shows its VM it counts twice, which errs towards fewer computers. (5) The engine refuses to
  stop a computer that is not running (`This computer isn't running.`, `stop_with_job`), so
  `CLI.Stop` treats a computer the list shows as stopped as done. (6) A fork that returns an error keeps
  its `forking` record until a list started after the error binds the exact name (deregister, then
  delete by id) or shows none (forgotten); the old test of a same-name twin was replaced, because the
  helper chose the name. A helper killed during a fork whose computer appears after the next start's
  first list is the remaining case, documented as the owner's to delete. (7) Cosmetic: `retain` has no
  return value, `type event = any` and the duplicated settler close and idle test are gone. The old
  watcher of an adopted runner's command is left: it ends with the command and its exit event is
  ignored by process identity. Tests: 86 (7 new), `go vet`, `gofmt`, `go test -race -count=3 ./...`
  pass, also with `TZ=UTC`; non-test Go is now 3045 lines.
- **Review round 4 fixes (7 Nov 2026).** (1) A fork refused because the name is taken
  (`Computer names must be unique.`, which the CLI prints as text on stderr with no code, so
  `CLI.Fork` marks it by that sentence) forgets the record and deletes nothing; the old rule would
  have bound and deleted the computer that had the name. Other fork errors still keep the record for a
  fresh list. (2) `pendingDemands` does not count a live record with no start in flight, no watched
  runner command and a computer that a list started after its fork or start (`seenFrom`) shows
  stopped, so a Mac restart does not hold macOS slots for adopted busy records; a start success moves
  `seenFrom` on, and a rebuild's `Reserve` list, which cannot be ordered, discounts nothing. (3)
  `JobStillRunning` sets the helper's own reason to `unknown`, not empty, so a restart before
  `JobStarted` is persisted adopts the record as busy instead of finishing it as an orphan and deleting
  a failed job's computer. (4) Docs: pending capacity wording, the restored-record rule, finishing
  records restarting at deregistration, and a dated note in the design. Each fix has an event-driven
  regression test; `go vet`, `gofmt` and `go test -race -count=3 ./...` pass, also with `TZ=UTC`.
- **Tests (no live GitHub, no engine, no computer):** `go vet`, `gofmt`, `go test -race -count=3 ./...`
  (also with `TZ=UTC`) in `app/SiloUI/runner`: 79 tests pass. The fakes complete at once and the tests
  drive the loop through events and wait for it to go quiet, so they do not sleep (one uses a 10 ms
  ready timeout). They cover capacity as a function of the list (macOS two-VM limit counting forks in
  progress, Linux count and recipe-file RAM), public-repository and public-runner-group refusal,
  success delete, failure keep, keepFailed count and days, deregister before delete,
  `JobStillRunningError`, failed deregistration retry, readiness retry and backoff, watchdog restart,
  the JIT value in the command's standard input and in no argv (a fake `silo` script), `silo drop`
  stopping first, config validation, the runner version refresh and its rollback, the ledger fsync, and
  a regression test for each review finding (retention before deregistration, a same-name twin and a
  failed fork, renamed computers and failed lists, a GitHub outage retried by the tick, a failed ledger
  write, a result arriving while finishing, recipe memory) and for restarts (idle orphan, job in
  flight, forking records, kept, interrupted keep). `cargo test -p silo-cli --locked`
  (synthetic GitHub values, shared target dir; run for the first build, this rewrite changes no Rust):
  all pass, including 3 new tests (`exec_stream.rs`:
  the input travels in the request body, not in argv; no step is recorded; `cli.rs` flag parsing;
  `recipes.rs`: the shipped recipes parse and pin the runner). `cargo fmt --check` clean;
  `test_ci_native_changes.py` 7 tests pass.
- **Not run:** the live check, `docs/forks-ci/checks/r1_runner_check.sh` (private test repository,
  Dev engine, needs scheduling); `actionlint` (not installed; the example parses as YAML); the full
  Rust workspace and the frontend suites (this phase changes only `cli/` and the Go helper).
- **Open:** (1) `go.mod` says Go 1.27.3 because `scaleset` v0.5.0 does; the brief said 1.25. (2) The
  in-guest client cannot run a command in a fork, and `silo-runner` does not install its token, so the
  example's fork/diff/fold steps need that install done separately. (3) PLAN.md, BUILD_LOG.md and
  DESIGN-QUESTIONS.md are copied from `claude/forks-phase-3` (they are not on `main`); identical text
  merges cleanly. (4) `silo-runner` is not built into the app bundle or released.
- **Live check, 8 Nov 2026 ([r1-runner](evidence/r1-runner/README.md)).** Linux recipe: 19 of 19 pass (run 2, 7 Nov). macOS recipe:
  the 7 Nov run was blocked by `silo-run-jit: No such file`. Cause: the recipe build checkpoints the *running* builder and a fork
  boots from the copied disk without the builder's memory, so on macOS the last writes (`silo-run-jit`, and half of the extracted
  runner: `bin/` was empty) were not on the disk. Fixed in the engine (first as a recipe step, see the follow-up below):
  `BUILD_VERSION` is now `silo-recipe-build-3` (older builds are rebuilt). Rerun with `R1_OS=macos` on the integration build
  `412e329c`: 18 of 19 pass (success job on `ci-silo-macos-r1-c69eb916`, deleted; failing job on `ci-silo-macos-r1-b8d9ee73`, kept
  stopped; no JIT configuration on any command line; restart reconcile; 0 runners at the end). The 19th failed because of the
  script (the recipe's own builder computer and the list header differed in the before/after comparison); the script is fixed
  and was not run again. The check script now takes `R1_OS=linux|macos`.
- **Review follow-up (8 Nov 2026).** The sync moved from the recipe into the macOS checkpoint capture: `Machine::flush_disk` runs
  `/bin/sync` (60 s limit, a failure refuses the capture before the pause) for every capture of a running computer, whatever its
  kind (manual, automatic, fork point, the recovery checkpoint before a Restore), because a fork discards saved memory for all of
  them. The recipe step is gone, `silo-recipe-build-3` stays. Linux needs nothing: its disk-only captures already require the
  guest flush and its full captures keep the memory. The check script now compares `silo list --all --json` by sorted
  (id, name, state), minus `ci-*` and the recipe builder (it needs `jq`); the `silo.go` fork comment names the fanout envelope.
