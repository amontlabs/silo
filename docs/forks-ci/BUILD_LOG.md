# Forks, sharing, CI runner, containers: build log

Plan: [PLAN.md](PLAN.md).

## Resume

- **Merged (8 Nov 2026):** F1 (#84), F2 (#88), C1 (#87), R1 (#83), F3 (#86).
- **Open phases:** F4 (export a computer that lives on another machine) and R2 (Linux hosts as runner
  machines), both in progress; container computers (C2 onwards) being designed.
- **Open follow-ups found during the build:**
  - A Linux fork leaves its fork-point checkpoint on the source (it was a manual checkpoint before F2 too);
    macOS removes its fork point with the last fork. Fixing it means the macOS release machinery on Linux.
  - The trimming half of `storage::start_monitor` (`periodic`) is not wired into `silod` since the engine
    split; F3 wired the sweep only. Empty 4 KB `silo-import-*` group folders can remain.
  - No macOS fork timing: creating a macOS source on the Dev engine failed ("did not accept SSH logins in
    time") during F1 and F2.
  - launchd refuses ad-hoc-signed Dev engine builds ("needs LWCR update"); live checks started `silod --agent`
    by hand.
  - `quiet::processes_match_by_exact_name_not_by_prefix_or_pattern` fails on main too (seen 8 Nov).
- **Linux KVM host:** the owner's test machine (10.77.77.2) did not answer on 7-8 Nov. Linux measurements ran
  on the GitHub KVM runner; each result names the machine it ran on.
- **Live-check repository:** https://github.com/0xpolarzero/silo-runner-e2e (private), used by the R1 checks.

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
- **Q4, guest image** (the import claim below was not accurate; see F3): exports stop embedding the guest image (no `--with-image`); import already
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
- **Container computers (decided by the owner, 8 Nov, after C1):** build them. An opt-in kind for Linux hosts,
  never the default, chosen knowingly for a dev-only machine with nothing sensitive on it. It has the same API
  and capabilities as a VM computer: everything a user can do with a VM computer (create, start, stop, exec,
  fork, checkpoints, diff and keep, secrets, GitHub access, ports and URLs, desktop and computer use, export and
  import, Connections) they can do with a container computer, through the same CLI, MCP tools, app screens and
  behaviour. The threat model states it is weaker than a VM.

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

- **Linux KVM, after** ([evidence](evidence/f2-after/linux-kvm-gh/README.md), GitHub runner, a different
  instance than F1's): fork of a running main 11.71 s (F1 11.03 s), fork right after `silo up` 11.95 s
  (10.14 s), `-n 4` 41.08 s (36.20 s), RAM per fork 1027 MiB (1027 MiB); one fork with a desktop 14.29 s and
  1022 MiB, without one 8.62 s and 387 MiB.

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
- **macOS sync before every capture (8 Nov, decided after the gpt-6.1-sol review of `0b29af74`):** the guest
  sync runs before every capture of a running macOS computer, including the before-keep, promote and undo
  recovery checkpoints. macOS checkpoints hold disks only, so without it a restore (undo) could lose recent
  writes just as a fork could. Cost: those operations wait for the sync (at most 60 s) and refuse when the
  guest does not answer it; keep already needs the guest to apply changes. Accepted: correctness of every
  saved point over the speed of a few. The `quiet::processes_match_by_exact_name_not_by_prefix_or_pattern`
  failure seen in the engine suite also fails on main (`569f6fea`) at the same assertion and is not caused
  by this branch.
- **R1 live check, run 4 (macOS, 8 Nov 2026): 19 of 19 pass**, with the engine sync of `0b29af74` and the JSON before/after
  comparison. Integration build `silo-f799cb60` (local merge of `forks-phase-3` and `runner-phase-1` on `origin/main`, release
  Dev bundle, engine started by hand), private repo `0xpolarzero/silo-runner-e2e`, `maxRunners: 1`, no macOS VM running before. The
  success job ran on `ci-silo-macos-r1-0f3b3af8` (deleted, runner gone), the failing one on `ci-silo-macos-r1-c1f4b46e` (kept,
  stopped, runner gone); no JIT on any command line (sampler: 26 sightings of `exec --stdin`). Direct check of the fix: a file
  written in a running macOS computer, a manual checkpoint right after, a fork of that checkpoint: the file (and a 4 KiB blob) is
  in the fork. Cleaned up: 0 runners, no `ci-*`/`e2e-*` computer, recipe build removed. Evidence:
  [`evidence/r1-runner/run-4-macos.txt`](evidence/r1-runner/run-4-macos.txt), `runner-4-macos.log`, `config-macos-4.yaml`.
### Phase F3: sharing a computer as a file (branch `claude/forks-phase-3`, 7 Nov 2026)

What changed, with the evidence for each claim. Test counts are from `cargo test --locked` (synthetic
GitHub configuration, shared target dir) and Vitest, run on 7 Nov 2026 on the Mac; none of it touched a
computer, the Dev engine or an app bundle.

- **`silo export` / `silo import`** (`cli/src/share.rs`, `cli.rs`, `render.rs`; `--json` like every
  command; the same job waiting as the other commands, `Engine::run_job`). `exports.create` gained
  `archivePath` (the exact file; the remembered-folder rule applies only without it) and there is a new
  job `exports.check`. `--device` is refused for both with "…work on this device for now, so --device
  cannot be used with them yet." No MCP tools: the MCP server has no owner-only tier, and both commands
  read and write files at paths the caller picks and the file can hold the computer's logins; reasons
  are in `docs/SiloUI-CLI.md`. Evidence: `cli/tests/share.rs` 10 tests (question text, no terminal
  without `--yes`, `--yes`, failed check, checkpoint by name, failed export/import reported, default
  import name, `--device`), 3 unit tests in `share.rs`.
- **Guest image by reference.** Verified first, and the claim in Decision Q4 was not accurate:
  `compare_loaded_descriptor` (`backup.rs`) compares the loaded snapshot's image with *the archive's own*
  manifest, not with this Silo's pinned image, so import did not refuse an archive naming another image.
  What replaces it: a Linux computer whose image reference (and recorded digest, when it has one) equals
  the pinned image that the runtime cache holds is saved without `--with-image` (`--with-parents`
  unchanged, including the retry without parents) and its manifest row says `imageEmbedded: false` and
  carries `imageDigest`, **always the sender's pinned digest** (`PinnedImageRef.manifest_digest`), also when
  the computer recorded none (before, such a row was matched by tag only). Validation requires a digest
  exactly on the rows that leave the image out. Import of a lean row now prepares the image **before the
  payload is extracted** (`prepare_restore_in_group` reads the manifest first, so an offline receiver fails
  before copying gigabytes; the download checks its own space in `download_and_publish`, and the extraction
  space check that follows sees what is left): the reference must be this Silo's pinned one (else "Update
  Silo…"), `preparation::ensure_image` prepares it as computer creation does, and the cached manifest digest
  must equal the row's `imageDigest`. **Format:** only an archive that needs format 5 (a lean row or a
  fork-time record) is written as format 5; any other is written as format 4 (header and manifest), so older
  Silo builds still read it and the changeset needs no warning. Format 4 files are still read; format 5
  fields in a format 4 file are refused. Evidence: `backup.rs` tests for pinned/other/none image with the
  digest and the format of each, digest required exactly on lean rows, prepare called first with the digest
  and with nothing staged, the retry without parents, import calling the preparation only for a lean row
  and stopping when it fails, format 4 written and read, format 5 fields refused in format 4;
  `backup_controller.rs` test for another pinned image. **Size before/after: not measured** (the unit tests
  use a fake runtime; the live check prints the size and takes `BEFORE_BYTES`).
- **Fork-time record travels.** Archive manifest `forkRecords` (computer, size, sha256, capturedAt,
  records, bytes), the gzipped stored manifest after the payloads; counted in the file length, hashed,
  verified by every full check, extracted for the selected computer on import and stored by
  `Store::put_imported_base` with no source and no origin (parsed first; a bad record is refused and
  stores nothing). Linux and macOS (and mixed) archives carry it. A failed import removes it. A computer
  without a record imports as its own, and `fold` without `--into` keeps its message. Evidence:
  `backup.rs` round trip, tampered record fails check and import, record for an unknown computer refused;
  `changes/store.rs` 2 tests; `backup_controller.rs` stores/forgets the record; `keep/tests.rs` two
  tests that read the imported record back and keep three-way into the receiver's main (kept; and
  refused as divergent with main untouched when the receiver changed the same file).
  **Hardening after review (7 Nov):** `put_imported_base` counts `records` and `bytes` from the parsed
  manifest and ignores the archive's numbers (the archive section keeps its fields; import no longer reads
  them), and a `capturedAt` that is not an RFC 3339 time between 2020 and a day from now is replaced by
  the import time (test with 1970, year 9999, text and empty). The compressed cap is 128 MiB, what a
  capture reads back from a computer (`Options::max_compressed_bytes`); the 512 MiB decompressed cap stays
  because a capture accepts the same (`Options::max_manifest_bytes`), so a lower one would refuse records
  the engine itself made. **Orphan record:** the record is stored before the computer is saved, and
  `discard_uncommitted_import` (also what relaunch recovery calls for a journaled identity) did not remove
  it, so an interruption between the two, or a failed save of the settings, left a record under an id no
  computer has; macOS relaunch recovery had the same gap. Both now call `forget_fork_record`, whose failure
  keeps the identity journaled for the next launch. Test: a stored record is gone after
  `discard_uncommitted_import`.
- **Login warning.** `credential_group` now matches `.claude/.credentials.json`, `.codex/auth.json`,
  `.gemini/oauth_creds.json` and `.gemini/gemini-credentials.json` (`gh/hosts.yml` was there). Claude Code and
  Codex paths are from the tools' own docs (code.claude.com/docs/en/authentication: Linux, mode 0600;
  developers.openai.com/codex/auth). **Gemini CLI, corrected after review:** `oauth_creds.json` is the legacy
  file; current gemini-cli migrates it into keychain storage and, with no secret service, writes
  `~/.gemini/gemini-credentials.json` (`fileKeychain.ts` in google-gemini/gemini-cli: `path.join(homedir(),
  GEMINI_DIR)` + `gemini-credentials.json`, written encrypted with mode 0600). Both are matched; the earlier
  claim that the path came only from third-party tools was wrong about the source, not about the legacy
  file. `login_label` names a login ("a Claude Code login (path)") and leaves out what every computer has
  (`/etc/shadow`, host keys, public keys, the desktop password). `changes::credentials::check` lists the disk
  with one change manifest capture (the `silo diff` walk, which runs a bounded script in the guest and
  reads a gzip back; cost = one capture of `/etc /root /home /usr/local /opt /srv`, seconds on a small
  computer, proportional to the number of files, and the computer must be running) and the stored record.
  Warn, never filter. **One "could not check" case (review):** a stopped computer (its disk is not listed;
  a check never starts it), a checkpoint export (only the record is checked) and a failed check are the
  same case, with the reason in `unchecked`. The CLI asks "Silo could not check <name> for logins
  (<reason>). Share anyway? [y/N]" (with "other logins" and the found ones in the same question when both
  apply), refuses without a terminal unless `--yes`, and with `--yes` reports the reason in `--json`
  (`unchecked`) and in the text output; the app shows the same sentence in its dialog before the folder
  picker, and a failed check asks too. Before, a stopped computer exported without a question and a failed
  check under `--yes` was reported as clean. Evidence: engine tests `ignore.rs` 2, `credentials.rs` 3;
  `cli/tests/share.rs` 12 (stopped computer asked and refused/accepted, failed check with `--yes` reported
  in JSON and text); frontend `computer-transfer.test.tsx` (failed check, stopped computer with no logins
  found, logins beside "other logins"), production bridge 2 new tests.
  **Tauri command:** `check_export_logins` compiled and its forwarding test passed
  (`cargo check -p silo-ui`, `cargo test -p silo-ui the_logins_check`) with empty placeholder files for the
  resources `build.rs` requires (`binaries/*`, `runtime/*`, git-ignored, deleted afterwards);
  `runtime:prepare` itself was not run (it builds MicroSandbox and Git from source with network access), so
  this proves the Rust compiles and the forwarding, not a packaged app.
- **Docs/changeset:** `docs/SiloUI-CLI.md` (commands table and a "Sharing a computer as a file"
  section, with what the person sees when Silo could not check), `docs/THREAT-MODEL.md` (boundary 6, including
  that an imported computer is untrusted until kept: `silo fold <imported> --into main` writes the sender's
  shell files, agent settings and crontab into the computer that holds the receiver's secrets, so read
  `silo diff` first), `app/SiloUI/.changeset/share-a-computer.md`.
- **Export file name:** `-o FILE` must end in `.silo-backup`; checked up front for Linux and macOS in
  `explicit_archive` (before, only the Linux write path checked, after the capture).
- **Live check, not run:** `docs/forks-ci/checks/f3_share_check.sh` (Dev engine, `e2e-share-*`; keeps
  the import into a second fork `e2e-share-target`, not the Dev main).

**Final review fixes (7 Nov; reviews: Fable LGTM, Opus LGTM after these).** An import now computes the
guest image to prepare a second time from the extracted archive and refuses ("the file changed while
Silo was reading it") unless it equals the image that was described and prepared, so a file replaced
between the two reads cannot load an unprepared image (test swaps the archive inside `prepare_image`).
A macOS import that left no computer propagates a failed fork-record removal and keeps the restore
identity for recovery instead of dropping the error (`settle_unsaved_macos_import`, with a test). `silo
export --yes` says "other logins" when some were listed. A disk listing that fails after the fork-time
record was read keeps the record's logins and reports the reason, and a Linux computer that cannot be
reached is no longer reported as stopped (3 tests in `changes/credentials.rs`). Run: engine `backup`,
`backup_controller` and `changes::credentials` tests (267 passed), `silo-cli --test share` (12 passed),
render unit test, `cargo fmt --check`; no frontend file changed.

Test results (7 Nov, after the review fixes): silo-engine 3123 passed, 8 failed, 31 ignored (the 8 are the
same `host_push::protocol_tests::*`, which panic "prepare the bundled Git runtime before testing" in this
checkout; none touches this change); silod 320 unit plus its integration tests, all passing; silo-cli 148
unit + 12 `share` tests, all passing; silo-client 16; silo-api 49. Frontend: typecheck clean, lint 0 errors
(existing warnings), Vitest 287 files / 2863 tests. `demo/` `tsc --noEmit` clean (node_modules deleted
afterwards). `cargo fmt --all --check` clean. New tests were written with their fixes, not run failing
first, except where a changed assertion shows the old behaviour (the CLI check-failure test).

**F3 live check (7 Nov, Dev engine, Mac) and the fixes it led to.** Evidence:
[f3-share](evidence/f3-share/README.md) (`raw-1-default/`, `raw-2-clear-session-bus/`). 18 of 22 and 19 of 21
checks passed; the failures were four product problems, all fixed afterwards (tests, not a second live run):

1. *Keep of an imported computer into the receiver's computer.* (a) `/home/silo/.dbus/session-bus/<machine-id>-1`
   is rewritten by every started computer, so any two computers started since their common ancestor
   conflicted on it ("divergent"). `changes/ignore.rs` now ignores `*/.dbus/session-bus` and
   `.xsession-errors*` (the X session's error log, the same kind of per-login file); test
   `per_login_runtime_files_are_not_a_change`. The other per-boot files under the home directory that the
   evidence shows (`.config/pulse/<machine-id>-*`, `.Xauthority`, `.ICEauthority`) were already ignored.
   (b) The `changed` refusal in run 2 was not a product fault: `silo fold` without `--reviewed` pins the keep to
   the digest of the last `silo diff` of the fork (`Engine::recall_review`, `ops::fold`), and that was S5's,
   before the probe deleted the session-bus files in the copy. The refusal's `report.reviewDigest`
   (`f70af3…`) equals the dry run's, while S5's `s5.json` has `142494…`: the fork did not change between the dry
   run and the keep, it changed after the review the keep was pinned to, and the refusal is the designed
   answer. `f3_share_check.sh` now runs `silo diff` on the copy right before S7 (the `CLEAR_SESSION_BUS` probe
   is gone). Not yet seen live: that the keep then succeeds; the probe is a rerun of the script on the Dev
   engine.
2. *The Silo CA named as a login.* `/etc/pki/ca-trust/source/anchors/microsandbox-ca.pem` is in
   `SYSTEM_CREDENTIALS`, as is the macOS equivalent, the proxy CA and its trust-probe leaf under
   `/Library/Application Support/Silo/NetworkBootstrap/` (written by `macos_computers/proxy/bootstrap.rs`; not
   checked live). A CA anchor a person adds is still named (test).
3. *Export free-space check.* `check_export_space` counted every snapshot and cache of the engine, once per
   selected computer, plus the computer's disk twice (in the estimate and as a capture on the store). It now
   counts, once per computer, its own snapshot group, its disk when a new capture is taken, and the image cache
   once when a file carries the image, plus the manifest margin and 1 %, for the working copy and the
   destination (and the 1 GiB reserve per volume). Ancestors kept in another computer's group are not counted
   (the check on the finished file uses its real size). Test `export_space_counts_the_computers_own_payload_once`
   (24 MiB of other snapshots and cache do not matter; the computer's own data does).
4. *`snapshots/silo-import-*` left behind.* An imported computer's group holds the archive's ancestors under
   the exporting computer's names, and dropping the computer queued only members named like Silo's own
   captures (`silo_member`), so the others stayed. `remove_deleted_snapshots` now queues every member of an
   import group (names valid for the journal; the planner still keeps what a record or a live computer's lineage
   uses). The startup/maintenance sweep (`recover_interrupted`) also queues the members of import groups that
   no checkpoint record on disk names, which clears the groups earlier drops left. Discarding an unfinished
   import already removed its whole group (`discard_import_group`). Tests (both failed before the change):
   `deleting_an_imported_computer_removes_its_whole_import_group`,
   `the_sweep_removes_import_groups_no_computer_owns`. The cause is from reading the code, not observed live.

- **Live rerun, 8 Nov 2026 (run 3, evidence in [f3-share](evidence/f3-share/README.md)).** On an integration build
  (`412e329c`) with `cf97e65a`: S2 no longer names the Silo CA (only the Claude Code login), S3 and S9 (checkpoint
  export) ran at 59 GiB free without a space refusal, but **S4 fails**: importing any export, even of a fresh
  never-forked computer, stops with `archive contains duplicate entry: layers/layer_87157fb5….raw` from the runtime's
  `snapshot load`. S5 to S8 were not reached, so S7 (keep into the receiver's computer) and the import-group
  clean-up of a dropped imported computer were still unseen. Failed imports left no group; six older
  `silo-import-*` folders were not cleared by the new engine.
- **Run 4, 8 Nov 2026 (evidence in [f3-share](evidence/f3-share/README.md), `run-4-rerun.txt`): 22 passed, 0 failed**
  (S9 `--device` skipped), S7 included; no `silo-import-*` left after dropping everything. Causes and fixes:
  (1) *Duplicate layer.* `msb snapshot save --with-parents` writes a layer twice when the capture's chain already holds its
  parent's head (any export after an earlier capture of the computer, such as F2's fork points), and `snapshot load` refuses it.
  Reproduced with one computer exported four times (the fourth file failed); exports now save the capture alone
  (`8794f629`; test `a_capture_is_saved_without_its_parents_so_no_layer_is_written_twice`, failed before). The old
  missing-ancestor retry and its two tests are gone. (2) *Groups left.* The six groups were empty folders by then (the
  8.4 GB was not reproduced). The sweep never ran: its only caller, `storage::start_monitor`, has had no caller since the
  engine split; `silod` now runs `recover_checkpoint_data` on a timer (`5aa8a1ae`). Dropping a computer now queues every
  member of its import group (`8794f629`), and a member released later (an imported copy shares its snapshot ID with the
  computer it came from and stays while that lives) is removed then (`05992ceb`, test
  `a_member_released_later_is_removed_through_the_runtime`). Silo does **not** remove the empty `silo-import-*` folder
  (`.group.lock`, `group.json`, about 4 KB each) MicroSandbox leaves; those can remain.
  **Still dead:** the trimming half of `start_monitor` (`periodic`, warm-computer trims) is not wired into `silod`; not
  changed here, it needs its own decision.
- **Folder removal dropped (review).** An earlier cut of the sweep unlinked empty `silo-import-*` folders from the
  filesystem after an unlocked scan, while MicroSandbox publishes under its own group lock, and its time budget excluded
  the inventory. Removed (`remove_unowned_import_group_shells`, `remove_empty_group_directory` and their tests):
  MicroSandbox has no command that deletes an empty group, and the folders are about 4 KB each.
