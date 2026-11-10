# Forks, sharing, CI runner, containers: build log

Plan: [PLAN.md](PLAN.md).

## Resume

- **Merged (8 Nov 2026):** F1 (#84), F2 (#88), C1 (#87), R1 (#83), F3 (#86), F4 (#93; its live check is
  the `linux-host.yml` two-account job).
- **Merged (8 and 9 Nov 2026):** R2 (#90; no job has run through `silo-runner` on a Linux host yet) and the
  container computers, CT1 to CT6 (#92, #94, #95, #97, #102, #103, #104).
- **Open phases:** none. The post-merge reviews are the sections of that name below.
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
- **Linux KVM host:** the owner's test machine (10.77.77.2) did not answer on 7-8 Nov (it answered `ssh` on
  10 Nov; nothing was run on it). Linux measurements ran on the GitHub KVM runner; each result names the
  machine it ran on.
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
### 8 Nov 2026: phase CT1 built (the gateway on Linux)

Branch `claude/containers-ct1`, pull request [#92](https://github.com/amontlabs/silo/pull/92).

- **Build.** The gateway (`silo-macos-gateway`) builds for Linux x86_64 and aarch64 next to macOS; the
  `aarch64-apple-darwin` gate in `microsandbox-runtime.mjs` is lifted and the Linux engine archive carries
  the binary. **The name is kept:** a rename would touch the macOS bundle, its signing, the engine's lookup
  and the runtime cache keys for no gain. The engine side needed no `cfg` change in the macOS supervisor,
  which is untouched; the container side is a new module, `silo_engine::computer_gateway`.
- **TAP carrier.** For a container fd 3 is a TAP (`IFF_TAP | IFF_NO_PI`) opened inside the container's own
  namespaces. `FramePump` takes a carrier: the datagram path (macOS) is unchanged; the TAP path reads with
  `readv` (a spare byte shows a truncated frame) and writes with `write`. The gateway checks with
  `TUNGETIFF` that the descriptor is a TAP without packet information and refuses anything else.
- **Parameterised.** Init takes `profile: "container"`: the published ports are the computer's own (up to 32)
  instead of the fixed 22/5900, and `github_hosts` may add `uploads.github.com`. A VM keeps both fixed
  rules. CA storage needed no new code: it goes through `keyring`, which is the credential file store on a
  Linux host; a test now proves it (0600 files in a 0700 directory, created once, retired).
- **Tests.** Gateway: 30 on Linux (Docker; the same suite passed on the runner) and on macOS (TAP adapter on a datagram-pair
  fixture, truncation, `TUNGETIFF` refusal, parameterised validation); `scripts/test-gateway.mjs` stages the
  patched checkout and runs them. Engine: 9 tests for `computer_gateway` (init and `ready` handling, update
  revisions and the close barrier, shutdown) and the file-store CA test. `cargo test --workspace` on macOS:
  all pass except the 8 `host_push::protocol_tests`, which need the prepared Git runtime (also skipped for F2).
- **Live check** on the KVM runner, no root step in the path: [evidence/ct1-gateway](evidence/ct1-gateway/README.md).
  25 of 25 checks (22 before the review fixes), with the script driving the gateway and with the engine starting it. Public HTTPS works,
  private addresses and the host's own address are refused, a fake token is substituted for
  `api.github.com` (401 "Bad credentials"), DNS works and a private answer is blocked.
- **Found on the way.** TLS is intercepted only toward hosts a secret is allowed for (the existing rule);
  the first version of the check expected interception everywhere and was wrong, not the gateway.
- **Review fixes** (Opus code review, Fable threat-model review):
  - **Fail closed.** The container gateway kept running after an update or stats exchange that timed out or
    was refused, and kept its revision, so a late acknowledgement was taken as the answer to the next
    update and a refused update (`invalid_revision`) could look applied: a secret revocation could look done
    while the secret was still substituted. Now any failed update or stats exchange stops the gateway and
    every later call answers "unavailable". The acknowledgement must also carry the protocol `version`.
  - **One control module.** `engine/src/gateway_control.rs` holds what the macOS supervisor and
    `computer_gateway` share: framing, the descriptor handoff, the `ready` identity checks (DNS = gateway,
    `/30`, gateway inside the guest's link), the CA generator, the acknowledgement rule and the child guard.
    The macOS supervisor uses it with unchanged behaviour (its tests pass unchanged). `Params` gains
    `gateway_ipv4`, which `ready` must report.
  - **Container host ports.** The container profile now requires every `host_port` to be 0 (the gateway
    picks); a requested port is refused at `init`.
  - **Live check.** A TUN device (`IFF_TUN`) handed over as the frame descriptor is refused with
    `invalid_frame_carrier`; the namespace process closes its own copy of the TAP after the handover; the
    usage log is parsed as JSON lines instead of counting strings; the gateway's own address is probed and
    refused.
  - **Caller contract for CT3.** `Gateway::stop()` (drop and `shutdown`) can take up to about 10 s for a
    hung gateway (5 s for the shutdown exchange, 5 s to exit), so CT3's caller must not drop a `Gateway` on
    an async worker thread. The gateway is not restarted after a failure; restart is planned for CT3.
  - **Docs.** The AppArmor sysctl is set by hand in CI; CT2 adds it to `silo host init`.
- **Not done.** aarch64 live run, UDP published ports, the container runtime (CT2).

### 8 Nov 2026: phase CT2 built (the container runtime core)

Branch `claude/containers-ct2`.

- **The seam.** `run_msb_process`, where the engine spawns the runtime, calls `containers::route` first:
  a command that names a container computer (or a `create` carrying the label `silo.runtime=container`) is
  answered by `engine/src/containers/` in MicroSandbox's argument and JSON shapes, and a `list` is completed
  with the container computers. Locks, the working account, change capture and the API above it are shared.
  Two more places know the kind: `Target::plan` (streaming guest commands start their own process, so the
  container's `crun exec` command is built there) and `create_computer_with_progress` (the image comes from
  the container image cache and the runtime label is added). The computer configuration gained
  `runtime: container` (absent for a virtual machine); `ComputerKind::Container` and
  `silo create --container` are the API and CLI side; the app lists the kind with "Less isolated than a VM".
- **Tools chosen** ([Reuse established tools](../../AGENTS.md)): crun 1.14.1 (Apache-2.0 or LGPL-2.1+,
  Ubuntu 24.04 apt, used in C1), skopeo 1.13.3 (Apache-2.0, apt) and umoci 0.4.7 (Apache-2.0, apt) to
  turn the pinned image archive into a root filesystem, the `oci-spec` crate 0.10 (Apache-2.0, already in
  the dependency tree through MicroSandbox) for the runtime configuration, `systemd-run --user --scope`
  for cgroup delegation. Silo checks them (`silo host init`) and installs none. Gaps that needed Silo
  code: a user-namespace holder that maps sub-ids, mounts the overlay and creates the TAP
  (`silo-ctr`, 600 lines of Rust over `libc`; podman's `unshare`/pasta would bring a container engine and a
  second network stack), and the Landlock/PID 1 shim (`silo-init`; no maintained single-purpose tool).
- **umoci runs inside the user namespace, without `--rootless`.** `--rootless` gives every file to the
  account (uid 1000 of the image became the account's uid, `/etc/shadow` lost its group). Run as root of
  `silo-ctr userns`, umoci keeps the owners as sub-ids, as in the image. Deleting a computer likewise runs
  `rm -rf` in that namespace.
- **Layout** (`containers/layout.rs`): the image once per config digest; a shim layer (`silo/init`,
  `/workspace`); per computer an upper and work directory, a `layers/` directory for sealed layers (CT4) and
  a record. The overlay's lower directories are the sealed layers newest first, the shim layer, the image.
- **Found on the way** (real Linux, Ubuntu 24.04 arm64, systemd user manager):
  - `crun exec` joins only the namespaces the configuration lists; the container shares the holder's network
    namespace, so an exec'd command landed in the host's. `silo-ctr enter` runs `crun exec` inside the
    holder's user and network namespaces; a test proves the exec'd command sees only `lo` and the TAP.
  - The overlay root takes the mode of the upper directory (a 0700 upper made `/` unreadable to other users).
  - The working-account setup assumes a workspace disk and a working `sudo`. In a container (`SILO_RUNTIME=container`)
    `/workspace` is a folder and no sudoers file is written: `no_new_privs` means `sudo` cannot work.
  - The engine runs with umask 077, which crun and every command in the container inherited: mount point
    directories and the `resolv.conf` and `hosts` files bound into the container came out private, so the
    working account could not resolve names. `silo-ctr` now sets umask 022 for everything it starts, and the
    shim layer and bound files are given their modes explicitly. Only the real engine on the KVM runner showed
    it (the live check prints the container's network state when a public host is unreachable).
  - A new container computer gets no default desktop from its image (the image includes one; a container has
    no desktop yet).
  - Silo's own root commands get seven capabilities (chown, file owner and mode, kill, setuid, setgid) inside
    the namespace, because creating the account needs them; other commands have none.
- **Not in CT2:** secrets, GitHub, published ports, SSH, terminals, desktop, checkpoints, forks, export (all
  refused with "Silo cannot <verb> a container computer yet"); CPU limits when the user manager does not
  delegate `cpu`; a container surviving an engine restart; a root filesystem size limit (the configured disk
  is recorded and reported, not enforced; planned in [CONTAINERS-PLAN](CONTAINERS-PLAN.md)); the app learning a remote host's platform (its editor offers the kind only for a
  device that reports `platform: linux`, which the Connections list does not report yet, so the CLI is the way
  to create one).
- **Review fixes** (code review by Opus, threat-model review by Fable 5.1; one mechanism each):
  - **Capabilities by kind** (`containers/capability.rs`). Terminal, editor (setup, key authorization,
    transport), SSH access, export and desktop reach the runtime by their own paths, not the argument shape
    `route` answers, so a container was handed to MicroSandbox. Each entry point now names its capability and
    calls one check by kind before any side effect, with the same "Silo cannot <verb> a container computer yet"
    message; the app hides the same actions from one list (`container-unavailable.json`, read by the engine
    test and the frontend). A runtime update may not change a computer's kind, and a container's desktop may
    not change (the installer would run inside it). A `--secret` on a container `create` is refused, and the
    engine no longer adds the GitHub secret and `GH_TOKEN` to a container's create (CT3).
  - **A read never mutates.** `status` only looks: a list refresh during a start used to stop the starting
    container (its holder exists before its gateway) and wait up to 25 s. A computer is marked starting before
    its holder is spawned; leftovers of an earlier engine are stopped once, by `reconcile` when the engine
    starts. The holder is identified by pid and its cgroup scope, not by the name `silo-ctr`.
  - **The recorded image.** Start and exec use the image digest in the computer's record, unpack it when it
    is the pinned one, and otherwise refuse with a clear message; a computer's upper layer is never mounted
    over another image. A partial unpack of another live engine is no longer deleted.
  - **Nothing is left behind by a timeout.** The command runs as the leader of its own process group
    (`silo-init --own-group`), `crun exec --pid-file` records it, and a timeout kills that group.
    Streaming execs go through the guest supervisor, which already ends its group. `crun exec` now gets the
    user's supplementary groups through `--process`. A `systemd-run` child is reaped on every error return.
  - **Untrusted account files.** `/etc/passwd` and `/etc/group` of the writable layer are opened component by
    component without following links, must be regular files, are capped at 1 MiB, and overlay whiteouts hide
    lower files.
  - **Hardening wording and details.** `setns` is denied outright; `systemctl` is located like the other
    tools; the holder logs one Landlock warning per start (unavailable or ABI below 4) and `silo-init` no
    longer repeats it on every command; the threat model states what Landlock covers, that an escape lands as
    the engine's account (engine socket, every other computer), that the writable layer has no size limit
    yet, and that CPU limits need a delegated `cpu` controller.
  - **Second re-check** (Opus). The desktop actions (`setup-agent-tools` included) and the computer-use
    approval check the capability before any side effect. Each exec gets its own private directory holding the
    pid file and the `--process` JSON (it carries environment values), removed when the exec ends: an
    `ExecGroup` drop for a waited exec, the plan's last holder for a streaming one (`Plan.files`, kept by
    `Streaming` until the process is gone). Leftovers are removed only when their command no longer leads its
    process group, so a waited exec running longer than 10 minutes keeps its pid file and its timeout still
    kills it. Root execs now carry the inheritable set matching ambient (the kernel needs it). The threat model
    says the ruleset applies on Landlock ABI 1 to 3. `reconcile` documents why a gateway it does not hold means
    a leftover: `silod` is the only process that starts containers for an account and holds the channel's
    `engine.lock`. The live check asserts that the root account setup of `silo start` (a waited root exec)
    worked and that no exec files remain.
- **Tests:** engine unit tests for the argument shapes, routing, the OCI baseline, the image cache, user
  resolution, the record and listing (macOS and Linux); 47 tests of `silo-container-shim`, including the real
  user namespace, overlay, TAP hand-over, PID 1 reaping and Landlock, run on Ubuntu 24.04; host prerequisite,
  API, CLI and frontend tests. Live: `linux-host-e2e.sh` with `SILO_E2E_CONTAINER=1` on the KVM runner (x86_64,
  Ubuntu 24.04, kernel 7.0): create, start, `uname -a`, a file written and read, listed as `container`, no
  capabilities, `no_new_privs`, seccomp, io_uring refused, nested user namespace refused, only `lo` and the TAP,
  a public host reachable, link-local, private, gateway and loopback addresses refused, stop, drop.

### 8 Nov 2026: phase CT3 built in part (network features for container computers)

Branch `claude/containers-ct3`, pull request [#95](https://github.com/amontlabs/silo/pull/95). Secrets, GitHub access, credential-use events, published ports and the URL proxy
work for container computers through the existing commands, API methods and screens. **SSH access, the editor, a
terminal and the in-guest client are not built and move to phase CT3b**: each needs a container-specific branch
outside `engine/src/containers/` and the capability list, which the brief said to stop and report. They stay on
the capability list (refused with the plain message, hidden in the app) until CT3b.

- **The seam moved one step.** CT2 routed a container at `run_msb_process`. Secrets and GitHub access reach the
  runtime through `spawn_runtime` (the shared `modify` calls skip `run_msb_process`), and `start` needs the
  secret material that `spawn_runtime_command` reads, so the route now sits in `spawn_runtime`, where every
  runtime launch is made. `containers::run` takes the material (`containers::Material`: the general secrets'
  values and the GitHub profile). The shared validation (`secrets_runtime::validate_material`), the plan
  (which secrets apply now and which at the next start) and the verification (`verify_config` on the saved and
  the active configuration) run unchanged: `containers/secrets.rs` answers the `modify --secret/--secret-rm
  --next-start` they issue and reports the result in the `network.secrets` shape MicroSandbox inspects. No policy
  was copied. A test runs `secrets_runtime::apply` against a container record.
- **What is kept.** The record gains `secrets` (name, allowed hosts) and `github`; values are never stored.
  The gateway gets the values at start and on each change, and the engine tracks which secrets the running
  gateway holds (`Running.active`): a secret assigned to a running container is recorded and applies at the next
  start, like a virtual machine's; a changed one and a removal apply at once. A removal asks the gateway only for
  a secret it holds (the gateway stops on a refusal). The guest environment gets `NAME=$MSB_NAME` for each
  active secret and the computer's own `--env` (so `GH_TOKEN=$MSB_SILO_GITHUB` from creation); CT2 had dropped
  the creation environment.
- **GitHub access.** A container is created with the same `--secret SILO_GITHUB:...` and `--env GH_TOKEN=...` as a
  VM (the CT2 gates are gone). The gateway starts with the cached profile (disabled when none) and the three
  GitHub hosts, and `apply_github_policy` reaches `Gateway::set_github_profile` through the same `modify`. Not run
  live: it needs a GitHub App sign-in.
- **Credential-use events.** The gateway writes to `sandboxes/<name>/logs/secret-usage.jsonl`, the file
  `computers.credentials` already reads for a Linux computer, so no reader changed. Dropping the computer removes
  the folder and retires its CA.
- **Fail closed, and restart.** The registry holds `Arc<Running>` per computer with its own lock, so a slow
  gateway exchange never blocks another computer's state read. `gateway_tracked` now asks the gateway whether it
  is alive: one stopped by a failed update makes the computer read as stopped, and `start` (which clears the
  leftover holder) brings up a new gateway. That is the restart: the holder gave its TAP descriptor away at start,
  so a gateway cannot be re-attached to the same network; a start is the clean way. `Gateway::close` stops a shared
  gateway without consuming it.
- **Ports and URLs.** The gateway's container profile gained `ports`, `port_add`, `port_remove` and `port_probe`
  (`macos-gateway/src/ports.rs`, over the network stack's `PortControlHandle`, which the MicroSandbox
  live-public-ports patch added and the gateway's single-tenant stack exposes). They are queries answered in
  order, not revisioned updates; a refusal (`port_rejected`) leaves the link in step, any other answer stops the
  gateway. A host port below 1024 is refused and at most 32 are published. The engine serves, per running
  container, the control socket MicroSandbox serves for a VM (`containers/ports.rs`, at the path `network.rs`
  already uses, `0600` in a `0700` folder) and forwards to the gateway, so `network.rs` and the URL proxy needed no
  change (only `socket_path` became `pub(crate)`).
- **The guest trusts the gateway's CA.** The first live run showed curl in the container refusing the
  certificate the gateway presents for `api.github.com` ("self-signed certificate in certificate chain"): a VM gets
  the CA installed by its runtime, a container did not. The first fix bound a bundle (the image's
  `/etc/ssl/certs/ca-certificates.crt` plus the CA) read-only over that path. Review found it broke the guest:
  `update-ca-certificates` renames a new bundle onto that path, which fails on a mount point (`EBUSY`), so an
  `apt` upgrade of `ca-certificates` or a JDK install failed and a guest could not add a CA of its own. Now only
  the computer's CA certificate is bound, read-only, at `/usr/local/share/ca-certificates/silo-gateway.crt`, and
  `start` runs `update-ca-certificates --fresh` as a Silo root exec once the container is up (a failure stops the
  container and says so). The bundle is the guest's own writable file, and the host reads nothing from the image
  for this any more, which also removes the host-side bundle read that could follow a link above the file in the image.
  Every command Silo runs in the computer gets the variables the runtime's guest agent sets in a VM
  (`microsandbox/crates/agentd/lib/tls.rs`): `SSL_CERT_FILE`, `REQUESTS_CA_BUNDLE` and `CURL_CA_BUNDLE` name the
  system bundle and `NODE_EXTRA_CA_CERTS` names the CA file. The CA is the third host file the container sees.
  Tools that ship their own bundle need their own setting. The live check now reinstalls `ca-certificates` in the
  container with `apt-get` and shows the HTTPS substitution through the gateway still verifies.
- **Review fixes.** A failed gateway exchange on a running container (the gateway stops, failing closed) now stops
  the container too, so the record, the interface and the processes agree; a `modify` writes its secrets into the
  record as it is after the exchange, not into a copy read before it (a concurrent assignment is kept); the
  `Running.active` comment says what the code does; the macOS gateway takes a port back when it cannot report the
  result of a `port_add`. Threat model: the container's difference from a VM is the escape boundary, not where the
  values live; the section names the three network differences (no passthrough hosts, a per-computer CA kept in
  the Linux credential file store, which tools trust it).
- **The probe runs off the pump's thread.** The first live run published a port but read it as `unknown`:
  `port_probe` waits for the guest's reply to a connection attempt, and that reply reaches the network stack only
  while the gateway's control loop, which is also its frame pump, keeps running. The probe now waits on its own
  thread and answers on a clone of the control link (one request is outstanding at a time). `port_add`,
  `port_remove` and `ports` need no frames and stay inline.
- **Live result.** Pull request checks all pass, `Linux computer on KVM` included (run 42504501686): the
  placeholder is what the guest sees, `api.github.com` answers "Bad credentials" with one `used` event and none for
  `example.com`, a published port answers through `8080.<computer>.silo.localhost` and on its loopback port and is
  closed after `unpublish`, and a secret taken from the running container records no further use. A secret must keep
  one computer, so the check gives it to a second container as well.
- **CT1 nit.** The CA handover moves the key (`std::mem::take`) instead of copying it with `to_string`, in
  `computer_ca` and in the macOS supervisor's `generate_ca`.
- **Why SSH is stopped.** The guest image has `openssh-sftp-server` but no `sshd`. `msb ssh serve`, the host
  server behind `ssh <name>.silo` and the editor's ProxyCommand, speaks MicroSandbox's exec channel and does not
  know containers; `ssh_access::spawn` and `editor` start it directly. Installing `openssh-server` in the image
  would not work under the baseline: sshd needs `setuid`, which the dropped capabilities and `no_new_privs`
  forbid, and the pinned image is shared with VMs. The simplest working design is a host-side SSH server for
  containers (an established library such as `russh`, Apache-2.0) that runs the login shell, `exec` and
  `sftp-server` through the container's `crun exec` plan, with the same managed keys. It also gives the terminal
  (`crun exec --tty` through `silo-ctr enter`; `terminal::command` builds `msb exec --tty` today and `parse_exec`
  refuses `--tty`). Both need a container branch in `ssh_access.rs`, `editor.rs`, `remote_access.rs` and
  `terminal.rs`.
- **Why the in-guest client is stopped.** The per-computer bridge folder can be bound into the container
  (`<runtime home>/sandboxes/<name>/runtime/silo-bridge` at `/.msb/silo-bridge`, the path the spool already
  uses), and a root agent would work: container root is the engine's account. A non-root agent writes request
  files as a sub-id with mode 0600, which the engine, an ordinary account, cannot read, and the stat-override
  sealing is a virtiofs feature a bind mount lacks. Fixing it means reading through the user namespace in
  `guest_client/spool.rs`, or group-readable modes plus a client change: a container branch outside the module.
- **Tests.** Engine (Linux in Docker and macOS): argument parsing of create and modify, record changes, the
  inspect shape, `secrets_runtime::apply` on a container, the gateway client's port exchange (fake gateway:
  ordering, refusal, wrong answer stops it), the gateway crate's `ports` module (32 gateway tests in the staged
  checkout). Live: `linux-host-e2e.sh` with `SILO_E2E_CONTAINER=1` now assigns a fake secret, restarts, checks
  the guest sees `$MSB_E2E_FAKE_TOKEN` and `$MSB_SILO_GITHUB` and never the value, gets GitHub's "Bad credentials"
  from `api.github.com` with a gateway `used` event and no event for `example.com`, publishes a port, opens it
  through the URL proxy and the loopback port, unpublishes it, and removes the secret from the running computer
  (no further `used` event).
- **Not done.** SSH, editor, terminal and in-guest client (phase CT3b, above); GitHub access live; UDP ports;
  aarch64 live run.

### 8 Nov 2026: phase CT3b built (SSH, the editor, the terminal and the in-guest client for container computers)

Branch `claude/containers-ct3b`. A container computer has SSH access, an editor connection, a terminal and the in-guest
client through the same commands, API methods and screens as a Linux VM. Fable's design, as decided:

- **SSH, editor, terminal: one seam.** The callers already spawn `<runtime executable> ssh serve NAME [--stdio | --host
  --port --authorized-keys --expected-machine-id ...]` (SSH access, the editor's `ProxyCommand`, Connections' remote
  guest) or `<runtime executable> exec NAME ... --tty` (terminals). `runtime::command_for(paths, name)` returns the
  `msb` command for a VM and `silod --container-runtime` for a container (`containers::runtime_invocation`; `silod`
  sits beside `msb`, where `silo-ctr` already does); the four callers use it and no caller branches on the kind. The
  capability gates for the terminal, editor and SSH access are gone from the shared list (it keeps export and the
  desktop). `silod --container-runtime` (`containers/cli.rs`) parses the same arguments (`containers/args.rs` gained
  `Command::SshServe` and `exec --tty`; a terminal with no command is `bash -l`) and runs either the server or
  `crun exec` on the caller's terminal.
- **The server** (`containers/ssh.rs`): russh 0.65.1 (Apache-2.0; the same library MicroSandbox's `msb ssh serve`
  uses), `default-features = false, features = ["ring"]` so the `rsa` crate (RUSTSEC-2023-0071) is not built; 65 new
  crates in `Cargo.lock`, none upgrading an existing one. `russh-sftp` is not needed: `sftp` is
  `/usr/lib/openssh/sftp-server` (already in the guest image, `openssh-sftp-server`; no image change) piped to the
  channel as the working account. Auth: public keys from the authorized-keys file the caller names (the editor's
  shared `ssh/authorized_keys` by default), read at each attempt, plain key lines only (a line with options is skipped),
  login `silo` only; host key `sandboxes/<name>/ssh/host_ed25519`; `--expected-machine-id` against the record's
  `silo.machine-id`. Sessions are `crun exec` as `silo` through the existing exec path (`silo-ctr enter`, the init shim,
  the process-group kill when a channel or connection ends); a pty is made by crun and handed to the server over a
  private `--console-socket`; client environment, agent, X11 and forwarding are refused. The listener prints
  `SILO_SSH_READY` (and the session lines) as MicroSandbox's does. The server runs apart from the engine, so the
  engine now publishes a computer's active secret names to `state/secrets.active` and the sessions get the same
  `NAME=$MSB_NAME` placeholders. A backend trait upstream (so both kinds share one server) is the better end state;
  not done here.
- **Keep-id.** The spool depended on virtiofs stat virtualization. In a container the working account (1001) is now
  the engine's own user and group (`containers/idmap.rs`): crun's maps are container `0..1000` to holder `1..1001`,
  `1001` to holder `0` (the engine's user), `1002..` to the same holder ids, so root is a sub-id and the agent's
  `0600` request files are the engine's on disk. `silo-ctr userns --keep-id N` (new) makes the same layout for the
  image unpack (umoci), and the boot layer and each upper directory are given to root through it (the agent must not
  own `/` or `/silo`). `Spool` takes the owner from the file's real uid and gid through the mapping when the bridge
  folder is a container's; it writes no override and keeps `0711` on the runtime folder instead. The unpacked image
  and the boot layer have a new directory name (`-keepid`), and a computer whose upper directory is owned by the
  engine's own user (an earlier build) is refused at start with "Recreate it". No migration (first container
  release). The share is the computer's runtime folder bound at `/.msb`; Landlock's writable list gained `/.msb`.
- **Found on the way** (KVM runner, x86_64, Ubuntu 24.04):
  - The container runtime gives the container's standard output and error files to the user its first process runs
    as. The holder's log is those files, and root is now a sub-id, so the second start could not open its own log
    ("Permission denied"). The log is now new each start and `0666` inside the private state folder.
  - A process started outside the engine's control group (a terminal from a login session, an editor's proxy) cannot
    join the container's group: `crun exec` failed with no message. Terminals and SSH sessions now run in a
    `systemd-run --user --scope`, as the engine's own commands do (`lifecycle::in_user_scope`).
  - `crun exec` with an inherited terminal exited 1 silently; the terminal command now makes the pty itself
    (`--console-socket`, as the SSH server does) and copies bytes between it and the user's terminal.
  - The root directory of the unpacked image belongs to root (a sub-id), which the engine may not move between
    folders: the namespace moves it. A unit name derives from the runtime home text, so a hand-made command line
    must use the home the engine passes.
  - `silo-init --own-group` no longer fails for a session leader (a terminal's command).
- **Threat model**: the container section says the working account is now the engine's user (an escape as the agent
  lands as the engine), why the namespaces still separate them, and describes the SSH server, SFTP, terminals and
  the share. Guides: [managed SSH](../SiloUI-MANAGED-SSH.md#container-computers),
  [in-guest client](../SiloUI-IN-GUEST-CLIENT.md#container-computers).
- **Tests**: argument shapes (`ssh serve` flags and refusals, `exec --tty`), authorized keys and key matching, the
  machine-identity refusal, term cleaning, the readiness line, an in-process russh client against the server (listed
  key accepted, another key, another account and a removed key refused, a session on a stopped computer errors, an
  environment and an unknown subsystem refused), the id extents and owner mapping, the OCI maps and `/.msb` mount, the
  upper-directory ownership refusal, the boot-layer name, the share modes, the published secrets, `command_for` and
  the proxy/terminal strings for both kinds, the spool's container owner and modes, `silo-ctr userns --keep-id` (maps
  and a real namespace test), and the frontend (a container is offered terminal, editor and SSH, not export or the
  desktop). Live: `linux-host-e2e.sh` with `SILO_E2E_CONTAINER=1` on the KVM runner passes: `silo ssh-setup`,
  `ssh NAME.silo-dev uname -a` (Linux), the account, an exit status, standard input, `pwd` in the home, an SFTP put and
  get of 300 kB (identical, owned by `silo`), `ssh -tt ... tty` (`/dev/pts/0`), `stty size`, an interactive shell, the
  terminal command on a pty (`/dev/pts/0`, `silo`, `/workspace`), the editor proxy in `--stdio` mode under a scripted
  `ssh` (an unlisted key and another account refused), and the in-guest `silo --in-guest list` as the non-root
  account after `silo guest-client install --binary`.

- **Review fixes (9 Nov 2026, PR 97).**
  - *Port forwarding.* The server answers `direct-tcpip` (`ssh -L`, `ssh -D`, the editor's forwarding), which
    MicroSandbox's server accepts, and only to the container's own loopback (`localhost`, `127.0.0.0/8`, `::1`, ports
    1 to 65535); anything else is refused. The connection is made by a new `silo-ctr connect --state DIR --host H --port N`,
    which joins the holder's user and network namespaces (`enter`'s code, now `join_holder`), connects, writes one zero
    byte, and relays standard input and output; the server starts it, waits for the byte, then accepts the channel, so a
    refused connection is an `open failed` for the client and the session loop never waits for it. The managed-SSH table
    no longer says forwarding is refused (remote `-R` still is).
  - *No stall under load.* `data()` awaited a bounded queue inside the session loop, which is also what takes the client's
    window updates: `ssh box cat < big > out` filled the command's stdin, the command's stdout filled the client's window,
    and the update that would have freed it sat behind the blocked loop. `data()` now never waits: the input goes to an
    unbounded queue that a writer task empties into the command. The first fix bounded that queue in bytes (32 MiB per
    channel, 64 MiB per connection) and ended a command that fell behind; the KVM check (60 MB both ways) then failed
    twice and passed twice on identical code. Ending a command for reading slowly is not what `sshd` does and would
    break `rsync`, `git` and large `scp` at random, so it is replaced by real flow control (below). A channel close or a
    dropped connection already killed the command's process group (the channel is dropped); the live check now proves it
    by killing a client.
  - *Login deadline.* A connection has 30 s from `accept` (the version exchange included) to log in, whatever the inactivity
    timeout; at most 64 connections are logging in at once; only a logged-in connection takes one of the 32 slots, so idle
    sockets can no longer lock SSH out under `--no-inactivity-timeout`. A client over the cap of logged-in connections
    is disconnected when it proves its key.
  - *Flow control (no command is ended for being slow).* russh 0.65.1 sends `WINDOW_ADJUST` itself, before it calls
    `Handler::data`, as soon as its view of the client's window is under half the target; the handler cannot delay it
    (`Handler::adjust_window` only picks the next target, after the packet is queued) and `Session` has no call to send
    one, so the window bounds what is in flight, not what is queued. Awaiting inside `data()` is no answer (the loop
    would stop taking the client's window updates, the stall above). So Silo vendors russh 0.65.1 with a patch
    (`src-tauri/vendor/russh`, `[patch.crates-io]` as for `tauri-plugin-updater`, `SILO-PATCH.md`; not upstreamed):
    `Handler::manual_window(channel)` (default false) makes russh count received bytes against the channel's window
    without granting more, and `Session::window_adjust` / `Handle::window_adjust` grant it (the handle's goes through the
    priority queue, so it is never held behind output waiting for the client's window). The server grants a channel's
    2 MiB window (`Config::window_size`) back as the writer task empties the queue into the command (in batches of 512 KiB
    and whenever the queue is empty), and at once for input nothing takes (before the command starts, after EOF, a command
    that closed its input, whose queued bytes the writer drops and gives back). A channel therefore queues at most 2 MiB
    and a connection at most 16 x 2 MiB; a client past its window gets its channel closed, as `sshd` disconnects it. The
    per-channel and per-connection byte limits and the "did not read its input" ending are gone. Tests (Linux container
    here, CI): the queue never passes the window, the window comes back only as the command takes the input, a command that
    stops reading gives it all back, and a real client sending 12 MiB to a command that is paused and then reads 64 KiB
    every 2 ms is held back, never ended, and delivers every byte with the queue within the window.
    `linux-host-e2e.sh` now runs the 60 MB `ssh cat` both ways five times (all five hashes must match) and sends the file
    to a command that reads 64 KiB every 10 ms, which must receive it intact.
  - *Memory bound and forwards (review round 3).* The window counted payload bytes only, but each `CHANNEL_DATA` was its
    own entry in an unbounded queue, so 2 MiB sent as one-byte packets was about two million entries (over 100 MiB for
    one channel). The queue is now one buffer per channel (`BytesMut` under a mutex, a `Notify` for the writer, which
    takes everything with `split()`): payload at most the window, allocation at most twice that, whatever the packet
    sizes. `direct-tcpip` forwards had russh's automatic window and a bounded queue the session loop awaited, so a
    target that stopped reading froze the whole connection (the shell too) and a bulk two-way forward could deadlock.
    A forward is now a command: manual window, input through `data()` into the same queue and a writer task (which the
    forward's task owns), output through `Handle::data`; the channel's own receiving end is dropped. Tests: one-byte
    packets to a command that never reads stay in one buffer of at most twice the window; a forward whose target never
    reads holds back its client while an echo forward and a new channel on the same connection answer; 16 MiB both ways
    through a forward with hashes (the in-memory transport must hold a window each way: russh writes without reading
    meanwhile, so two russh ends sending in bulk over a small buffer wait on each other). `linux-host-e2e.sh`: 60 MB
    both ways through an `ssh -L` forward (hashes), and a shell command on the same connection that still answers while a
    forward's target does not read. Output is still held only by the client's window: a client that stops reading one
    channel stops the loop from taking the connection's other output (russh does not read its handle queue while a
    channel has pending data), which the guide says. The patch draft for russh is
    [the upstream draft](../v1/upstream/russh-manual-window.md).
  - *russh is watched by hand.* While russh is vendored (`src-tauri/vendor/russh`), Dependabot and RustSec no longer see
    it, so its releases and advisories are checked by hand until the patch is upstream (`SILO-PATCH.md` says the same).
  - *Signals* in an SSH `signal` request go to the exec's process group (`ExecGroup::signal`, the same leader check as the kill).
  - *Root's in-guest answers* are a documented difference, not changed: the engine cannot give a file to a sub-id without a
    helper in the holder's namespace, and a looser mode would show the answer to any guest user that learns the id.
    A root process with `CAP_DAC_OVERRIDE` (Silo's own root commands) reads it; one without gets "permission denied".
    Stated in the in-guest client guide and pinned by a test.
  - *Storage.* `prune_storage` (at engine start and before an unpack, so the space check counts only what is needed)
    removes unpacked images no computer's record names (the pinned image stays) and, with no container running, boot
    layers of other init shims.
  - *Terminal start* keeps `crun exec`'s error output and returns at once when it ends before handing over the terminal
    (it waited 15 s with no reason); the listener logs an `accept` error and tries again after 50 ms, doubling to 2 s.
  - *Threat model.* SSH refuses a login unless `silo` in the computer's account files has the id 1001 (never 0) that the
    keep-id mapping gives the engine's user. The container section names the `/.msb` share as the one engine-owned
    folder in the mount namespace and no longer says nothing is added to the account. An upper directory is current only
    if a sub-id of the account (from `/etc/subuid`) owns it. `Spool::new` takes the `KeepId` from the worker
    (`containers::keep_id_for(home, name)`), so the spool does not know about kinds.
  - *Tests.* Unit tests for each, run in a Linux container here and in CI; `linux-host-e2e.sh` gained forwarding (`-L`,
    `-D`, refused destinations, a port the host does not serve), a 60 MB file through `ssh cat` both ways with hashes
    (five rounds, and a slow reader; see the flow-control item), a killed client, and 40 idle sockets with a login in between.

#### Post-merge review: container basics (CT1 to CT3b, 10 Nov 2026)

Review of the gateway on Linux, the runtime core, secrets, ports and SSH for container computers as they are on
`main` after CT6 (`engine/src/containers/`, `container-shim`, `computer_gateway.rs`, `gateway_control.rs`, the
vendored russh patch, checked against the published 0.65.1 source).

- **Fixed: ending a command could signal a process that was not the computer's** (`hold.rs` `ExecGroup`). The
  pid file of an exec outlives its command: an SSH channel is closed after its command ended, a terminal ends,
  and a client may send a `signal` request on a channel whose command is over. The only check was that the pid
  leads a process group, so once the host had given the number to another group leader (a shell's job, a daemon,
  another computer's command) that group was killed or signalled. The signal is now sent only while the pid is a
  process of the computer's own scope (`/proc/<pid>/cgroup`, the check the holder already has). Test:
  `a_pid_that_now_belongs_to_a_process_outside_the_computer_is_never_signalled`.
- **Fixed: an exec that was starting could lose its pid file** (`hold.rs` `exec_leftover`). Each new exec removes
  the directories of execs whose command is gone, and "gone" meant "the pid does not lead a process group".
  crun 1.14.1 writes the pid file as the command goes on to `execv` (`container.c`, `wait_for_process` after the
  child's go-ahead byte), which is before `silo-init --own-group` has run, so an exec starting at the same time
  could take a live command for a leftover and remove its files; no timeout, closed channel or signal could end
  that command afterwards. A directory with a pid is now a leftover only when no process has that pid. Test:
  `an_exec_whose_command_has_not_taken_its_group_yet_is_not_a_leftover` (failed before).
- **Fixed: the folder of a terminal's socket followed the umask** (`ssh.rs`, `cli.rs`). crun hands a terminal's
  controlling side to a socket the SSH server or the terminal command listens on, and whoever connects first
  gives the session its terminal. The folder was made `0755` under the usual umask, and the socket open to a
  group or to everyone under a looser one (an editor's proxy and a terminal start from a login session, not from
  the engine). It is now `0700` explicitly (`hold::console_directory`). Test:
  `the_folder_of_a_terminals_socket_is_the_accounts_alone` (failed before).
- **Fixed: the threat model said a container's SSH has no port forwarding.** The server answers local forwards
  toward the container's own loopback since CT3b's review; the section now says what is forwarded and refused,
  and describes the two fixes above.
- **Not changed.** A client that disconnects in the moment between crun starting a command and that command
  leading its group leaves the command running (the kill finds no group yet); it is the client's own command and
  the moment is the one above. `silo-ctr enter` trusts `outer.pid` and the callers check the holder first, as
  documented.

### Phase F4: sharing a computer that lives on another machine (branch `claude/forks-phase-4-transfer`, 8 Nov 2026)

Decision Q5 built as designed by the owner's brief and Fable: the export runs on the owning machine,
its file is copied over Connections, the owner's copy is removed. Test counts are from `cargo test
--locked` with synthetic GitHub configuration and a shared target dir, run on 8 Nov 2026 on the Mac;
no computer, Dev engine or app bundle was touched.

- **Why a stream.** The managed key is `restrict,command="... --remote-bridge"`, so `scp`/`sftp` cannot
  reach the owner, and Files' `sftp` only reaches guests through `guest.ssh`. A new Stream method
  `exports.file` carries one file, like `guest.ssh` (`engine/src/remote.rs` `METHODS`, served by
  `serve_file_stream`; the handshake's capability list is the old-owner check).
- **Transfer area** (`engine/src/transfer_area.rs`): `<engine dir>/transfer/`, 0700 and 0600. Names are
  owner-made `x-<32 hex>.silo-backup` (export, read once through an exclusive `.used` marker, links not
  followed) and `i-<32 hex>.silo-backup` (import, never readable); `parse_name` is the fixed pattern. A write
  declares size and sha256, is refused at zero, past the cap or without room (`size + 64 MiB` beyond the
  bytes other writes of the process have reserved; see "Review fixes"), goes to `.part`, reads exactly the declared bytes and is renamed only when the
  hash matches; every other end deletes the `.part`. **Cap: 16 GiB.** Not the Files 4 GiB (a computer
  export can exceed it) and not the engine's export limit, which is 8 TiB; 16 GiB is a deliberate middle
  that keeps the time budget finite. Remove is `exports.discard`; the sweep empties the area when
  `silod` starts and removes entries older than a day on the storage timer (`timers::sweep_transfer_area`).
- **Owner jobs, one variant.** `exports.create` takes `transferArea` (the owner picks the name; the job's
  `archive.transfer` holds `{name, bytes, sha256}`, hashed after the write: `backup_controller::describe_transfer`),
  `exports.import` takes `transferName` (resolved to an import name that is in the area, otherwise the
  ordinary import: the archive checks are the boundary). New API methods: `exports.discard` (owner command)
  and `exports.transfer` (controller job, `deviceId` required). The check, create and import are asked on
  the owner through the existing routing; the CLI's `ROUTED_METHODS` lists them.
- **Controller job** (`silod/src/methods/checkpoints/export_transfer.rs`): `receive` writes a new 0600 `<path>.part`,
  links it to the path when the hash matches and removes it on any failure; `send` hashes the local file, then sends. Progress is the Files
  `TransferProgress`; cancel is the job's; the Files budget (45 s plus 64 KiB/s) is enforced by a watchdog
  that closes the stream. An owner without `exports.file` is refused with "The other machine's Silo is too
  old to share computers over Connections. Update Silo there." by the job and by the controller's routing of
  `exports.create`/`exports.import` that use the area.
- **Review fixes (8 Nov 2026).** Findings of the review of the first version, each with a test:
  - *Disk overfill.* `begin_write` only read `statvfs`, so concurrent writes each passed. A
    process-wide reserved-bytes counter per area folder is checked and raised under one lock and
    released by the `Writer` on rename or drop. Writes are not serialized: the stream budget of 192
    (`remote.rs` `STREAM_LIMIT`) bounds how many are open, the reservation bounds the disk. A write's full
    size stays reserved while its `.part` grows, which errs on the side of refusing. Docs said "counting
    writes already promised" and "one transfer at a time"; they now say what the code does.
  - *Import file lost on a failed poll.* `import_to_device` discarded the `i-` file on any error. It now
    discards when the import succeeded or `Engine::last_job_finished` says the job is finished; a failed
    wait leaves the file (the sweep removes it) and the re-attach hint stays valid.
  - *Over-cap export.* The owner removes the `x-` file when `describe_transfer` fails (too large,
    cancelled, unreadable); the size is read from the file before it is hashed and the error names the
    file and size. The up-front refusal from `estimate_export_bytes` (before compression, whole store
    and cache when the image travels) is gone: it could refuse an export that would fit. Instead the
    export stops itself: `BackupRequest::size_cap` (set to the area's 16 GiB for a file in the area)
    is checked in `create_backup_with_token` after each computer's disks are written, and the error
    names that computer and the cap. This is not a mid-write abort of the tool that writes a
    computer's disks (an external command), so one oversized computer is still written to the
    working copy before it is refused; nothing is assembled, no file is left at the destination, and
    the working copy is dropped. The after-write check stays. The estimate still feeds the free-space check.
  - *Export files 0600.* `describe_transfer` sets 0600 on the export, so "files 0600" is true for both
    kinds.
  - *Receive wrote to the final path.* `export_transfer::receive` writes `<path>.part` and puts it at the
    path only when the hash matches; an existing file is refused before the copy and nothing is replaced.
    The first cut used `hard_link`, which exFAT/FAT32/vfat and many SMB shares refuse, so every copy there
    failed at the end. It now uses `transfer_area::publish_without_replacing`: the kernel's exclusive rename
    (`renameatx_np` `RENAME_EXCL` / `renameat2` `RENAME_NOREPLACE`), else a hard link and removing the `.part`,
    else an existence check and a plain rename. The fallbacks are tested with injected errors. A `.part`
    left by a crash (a regular file of this account) is removed once and the copy starts again; anything
    else there is named in the error.
  - *Broken pipe.* `push` reads the owner's error line when a write fails and returns the owner's reason.
  - *Client-chosen names.* `exports.create` refuses a destination or archive path inside the area
    (`Area::contains`, links followed); only `transferArea: true` names a file there.
  - *Marker on a failed open.* `open_for_read` removes the `.used` marker when the `O_NOFOLLOW` open fails.
  - *Sweep order.* `silod` empties the area before `start_connections`, not after the bridge is up.
  - *Why 16 GiB* is in `docs/SiloUI-CLI.md`.
- **CLI** (`cli/src/share.rs`): `--device` export checks the output name first, creates the export in the
  owner's area, copies it with a local `exports.transfer`, then discards the owner's copy whatever the copy
  did; import sends first, imports from the received name, and discards once the import is finished. A failed
  discard is a warning.
- **Docs/changeset:** `docs/THREAT-MODEL.md` (boundary 6, "Sharing a computer that lives on another
  device"), `docs/SiloUI-CLI.md`, `docs/SiloUI-CONNECTIONS.md`, `app/SiloUI/.changeset/share-across-devices.md`.
- **Tests.** Engine `transfer_area` 19 (names and roles, single use, links, `.part`, hash and short/long
  streams, abort, size/cap/free space, remove, sweep, the stream over socket pairs both ways), `remote`
  170 (the method lists), `backup_controller` 1 (`describe_transfer`). `silod` `methods::` 244 (a controller
  job against the real owner side over socket pairs: receive, send, bad hash, refused name, cancel of a
  stalled copy, old owner, request validation; owner `exports.discard`, `exports.create` with the area,
  `exports.import` from the area; the routing gate; the sweep). `silo-cli` 150 unit + `tests/share.rs`
  17 (the device flows, the discard on every path, the order of requests and which job is followed on
  which device). `hosts-e2e.sh` unit tests 18 (the sharing steps and a file left in a transfer area).
  `silo-api` 49 (schema and requests regenerated).
- **Live check:** `hosts-e2e.sh` with `SILO_E2E_SHARE=1` on the two-account KVM job
  (`linux-host.yml`): see the pull request for the run. **Not covered by any test:** a real `ssh` hop with
  the restricted key (the unit tests replace it with socket pairs), the Mac, and an exported computer
  larger than the runner disk.

### R2: Linux hosts as runner machines (branch `claude/runner-phase-2`, 8 Nov 2026)

- **Built.**
  - *Packaging:* `package-engine.py` builds `silo-runner` (`go build`, `CGO_ENABLED=0`, `GOOS=linux`, `GOARCH` of the
    platform, `-X main.version=<engine version>`) and puts it in both Linux archives as `silo-runner`, with the Go modules'
    licenses (checked in under `runner/licenses/`) in `resources/silo-runner-licenses/` and a notices section. The ELF
    machine check covers it. The jobs that package a Linux archive (`linux-host.yml` x2, `containers-c1.yml`,
    `engine-release.yml`) install Go from `runner/go.mod`; the release verification runs `silo-runner version`. Not in the macOS
    engine archive (an embedding-app format; a Mac builds the helper with `go build`).
  - *Service:* `silo-runner service register|unregister` (`service.go`) writes `~/.config/systemd/user/silo-runner.service` with
    `After=`/`Wants=` the engine's unit (`-engine-unit` for a Dev engine), `Restart=on-failure`, `UMask=0077`,
    `KillMode=mixed`, an optional `EnvironmentFile=-%E/silo-runner/env` for the token. **Chosen over a documented unit file**
    because the unit needs canonical, systemd-quoted paths and a refusal to overwrite foreign files, the same reasons
    `silod --service register` writes its own; about 150 lines, unit-tested. A user unit runs as the account of the user
    manager, which is the account of the engine's socket; `silo-runner` also refuses euid 0.
  - *Capacity:* on a Linux host (`GOOS=linux`, no `silo.device`) a `linuxBudget` field left out defaults to three quarters
    of `MemTotal` and one computer per CPU. **Chosen over requiring it** so the safe setting does not depend on remembering
    to write it; `MemTotal` and not `MemAvailable` so a restart with computers running computes the same budget. A budget
    below one computer of the largest recipe, a `macos` scale set, and an unusable `/dev/kvm` (missing, or no permission:
    what to do is in the message) stop startup. `silo-runner check -offline` does the host part with no GitHub; an empty
    `tokenEnv` variable is named before any call.
  - *Recipe:* `silo-linux` pins `RUNNER_SHA256_ARM64` and `RUNNER_SHA256_X64` (the digests of `linux-arm64` and `linux-x64`
    2.338.2, from the release API) and picks one by `uname -m` while building. `ParsePin`/`WithPin` and the refresh handle
    both: each digest comes from its own asset of the same release, and a release missing either changes nothing. The
    Rust recipe test (`cli/tests/recipes.rs`) expects the two digests.
  - *Docs:* CI runner guide "Linux hosts" (install, config, service, journal, capacity), threat-model bullets (the
    credential on the host, the journal readers, root, KVM, both pins, the Linux budget), engine and release docs,
    `examples/config-linux.yaml`, changeset (`silo-ui`: minor).
- **Live check: what was and was not run.**
  - *Not run:* `docs/forks-ci/checks/r2_linux_runner_check.sh`, the whole flow (runner for the machine's architecture, a
    computer per job on KVM, deleted on success, kept on failure, runners removed at GitHub, the service and its journal).
    It needs an admin token that CI must not hold, and the owner's Linux machine (10.77.77.2) was offline. **No job has
    run through `silo-runner` on a Linux host, and the x86_64 runner recipe has never been built.**
  - *Run by CI on KVM (no GitHub), `scripts/runner-host-e2e.sh` in the Linux host workflow:* the archive's `silo-runner` runs
    on that architecture and reports the engine version; `check -offline` opens the real `/dev/kvm` and sizes the budget
    from the host; a `macos` scale set is refused; `service register` against the real systemd user manager gives a unit
    that is enabled, ordered after and wanting the engine's unit, passing `systemd-analyze --user verify`; the helper's
    log (it stops for lack of a token) is in the user journal under this account's uid; starting the helper starts a stopped
    engine; `service unregister` removes the unit and leaves the engine running. A fake GitHub was not built: the
    runner program inside the computer needs the real service to reach "Listening for Jobs", so a stub of the scale set
    endpoints could not honestly exercise a fork per job or runner deletion, which the Go fakes already cover.
    (First run happens when the PR's checks run; see the PR.)
- **Tests:** Go `go vet`, `gofmt`, `go test -race ./...` in `runner/`: 115 tests pass, 24 of them new (`host_test.go`,
  `service_test.go`, and the architecture cases in `misc_test.go`): the service unit text and its quoting, register and
  unregister calls, refusal of root and of foreign unit files, the default budget and its sources, KVM errors, the
  macOS-scale-set refusal, the shipped recipe's two pins, the refresh moving both digests from their own assets, and a
  recipe whose digests do not match its download. `test_package_engine.py` (the archive holds `silo-runner` and the licenses,
  the ELF check, the Go build command per platform), `test_linux_host_workflow.py`, `test_engine_release_workflow.py`,
  `test_workflow_pins.py`, `test_ci_native_changes.py` (no path change needed) pass; `cargo test -p silo-cli --test recipes`
  passes with synthetic GitHub values; `shellcheck` is clean on `runner-host-e2e.sh`. Both Linux binaries were
  cross-built on the Mac and are ELF x86-64 and AArch64. The classifier is unchanged: `runner/` stays skippable except
  `recipes/`.
- **Review fixes (round 1, same branch).**
  - *Unit could not find `silo`:* the user manager's `PATH` lacks `~/.local/bin`, so `silo.path: silo` failed every call and the unit
    restarted every 10 s. `service register` now resolves `silo.path` (`exec.LookPath` from the registering shell; an absolute path
    must be an executable file; a relative path with a slash, or anything unresolvable, is refused) and writes it into
    `ExecStart` as `run -config <file> -silo <absolute path>`; `run` takes `-silo`. Example config uses an absolute path and names
    `silo-dev`. Tests: unit text, `ResolveSilo`, register refusal writes nothing. `runner-host-e2e.sh` also greps the unit.
  - *Old single-arm64 recipe on an x86_64 host:* `ApplyHost` (start and `check`) parses each installed Linux recipe's pin and
    refuses a single-architecture pin other than the host's `GOARCH`, naming `silo-runner install-recipes -replace`. New flag
    `-replace` moves the existing recipe to `<name>.yaml.bak` and writes the shipped one. Upgrade notes in the guide.
  - *0600 was only documented:* `SecretFileWarnings` warns (log at start; `warning:` lines in `check`, also `-offline`) when the App
    key or `~/.config/silo-runner/env` is group- or world-readable, as ssh does for a key; it does not refuse. Threat model reworded
    to "the guide makes them 0600 and the helper warns otherwise".
  - *Default budget ignored cgroup limits:* the default comes from `MemTotal` and the CPU count only (a cap read from the helper's
    own cgroup missed a `MemoryMax=` on silod's unit and could shrink the budget wrongly). Docs say that where the engine or the
    user's slice has a memory limit, or in a container or VM with less memory than it reports, `linuxBudget` must be set; the stale
    comment on `LinuxBudget` (zero now means the host default) is fixed.
  - *Packaging:* `go_build_runner` no longer leaves its `mkdtemp` directory (`main` builds in a `TemporaryDirectory`); the licence
    glob skips dotfiles and directories. Python tests for both.
  - *Tests:* `go vet` (also `GOOS=linux` amd64 and arm64), `gofmt`, `go test -race ./...`; `test_package_engine.py`,
    `test_linux_host_workflow.py` pass. Still no GitHub run and no Linux machine.
- **CI: `Linux computer on KVM` failed at `silo create` ("Silo could not download the VM image"), not caused by the runner step.**
  Runs 42465256087 (attempts 1-3) failed at `== Creating e2e-hosts-ci` after about 40 s. The VM image is fetched from
  `github.com/amontlabs/silo/releases/download/guest-ubuntu-24.04-v5/image-amd64.tar.gz` (`guest-image/image-lock.json`); that
  asset exists (361 MB, `uploaded`), but at 10:19 UTC every `github.com/.../releases/download/...` URL, including an unrelated
  repository's, answered HTTP 504 from outside CI while githubstatus.com reported all systems operational. Other branches
  failed the same step in the same window with no runner step at all (`claude/forks-phase-4-transfer` run 42469940031 at
  10:14:29 UTC, whose workflow has no `runner-host-e2e.sh`; `claude/containers-ct1` run 42466752224 at 09:36 UTC), while
  run 42467219772 passed at 09:26: a flaky release-download endpoint, not state the runner step leaves behind. The runner
  step only restarts the engine through `Wants=` with the same user-manager environment, home and image cache dir as the
  engine the register step started, and the engine journal shows nothing between the restart and the stop. The create step
  now retries a download failure (3 attempts, 30 s apart) so a brief GitHub outage does not fail the job.
- **Open:** (1) the live check above; (2) `silo-runner` is not in the macOS archive; (3) one helper per machine, so two machines
  serving the same recipe need two labels; (4) the default budget ignores memory the host's other services use, so on a
  shared machine set `linuxBudget`.

#### Post-merge review: CI runner (R1 and R2, 9 Nov 2026)

Review of `app/SiloUI/runner`, its CI wiring and its docs as they are on `main` after the container phases.

- **Fixed: the command of a runner that never listened was never reaped** (`fleet.go` `onUpDone`). When the
  runner did not print "Listening for Jobs" within `readyTimeout`, its `silo exec` command was still running and
  nothing drained its output or waited for it: the helper interrupted it after deregistration and dropped it, so
  every such failed start left a zombie process and the goroutine `os/exec` keeps per command, until the helper
  exited. The command is now watched whenever the start returned one, ready or not. Test:
  `TestTheCommandOfARunnerThatNeverListenedIsReaped` (failed before).
- **Fixed: a running container computer did not count against the Linux budget** (`capacity.go`). CT2 made the
  command line list container computers with the kind `container`; capacity counted only `linux`, so on a Linux
  host the helper could start virtual machines into memory a container computer already used. Everything that is
  not a macOS virtual machine now counts as a Linux computer. Test:
  `TestARunningContainerComputerCountsAgainstTheLinuxBudget` (failed before).
- **Fixed: docs that said the default budget reads cgroup limits.** The guide's configuration table and the
  threat model said the default memory was the smaller of the machine's and a cgroup limit; the code reads
  `MemTotal` only (the cgroup cap was dropped in R2's review). Both now say so. The R2 entry above had its
  "Built", "Live check", "Tests" and review bullets twice, the second copy with the dropped cgroup behaviour; the
  copy is removed. The public-repository refusal said "this Mac" on a Linux host; it says "this machine".
- **Checked, no change:** one event loop writes the records and the ledger; the phases and the by-id rule; the JIT
  configuration only on standard input (the CLI's `exec --stream --stdin` sends it in the request body and prints
  a reattach hint on interrupt, so the runner in the computer outlives the command); one label per scale set;
  `DisableUpdate` set on create and repaired on an existing set; the watchdog (the `scaleset` listener calls
  `Scale` once per poll, also with a nil message); scale-up from a fresh list with pending demands; restart
  adoption; delete on success and keep on failure; no checkpoint call anywhere in the helper; the private
  repository and runner group check failing closed; the docs' statement that GitHub's own secrets and
  `GITHUB_TOKEN` are not placeholders.
- **Left for the owner:** (1) the private-repository check runs at start only, so a repository made public while
  the helper runs is served until the next start; (2) a second Ctrl-C does not end a slow shutdown (the signal
  stays caught); (3) the Resume section above still listed R2 as in progress (corrected on 10 Nov by the
  container sharing and docs review); (4) no job has run through
  `silo-runner` on a Linux host (unchanged from R2).

### 9 Nov 2026: phase CT4 built (checkpoints, forks and restart for container computers)

Branch `claude/containers-ct4`. A container computer has checkpoints, forks (`-n N`, `--from-step`), quiet forks, diff,
keep, undo, steps and retention through the same API, CLI, MCP and app as a Linux VM; the CT2 gaps (an engine
restart, the disk size) are closed. Design as approved: [CONTAINERS-PLAN](CONTAINERS-PLAN.md).

- **The seam answers MicroSandbox's operations in its shapes** (`containers/snapshots.rs`, `containers/args.rs`):
  `snapshot create/list/verify/remove/head`, `restore G:N --name X`, `pause`, `resume` and `stop --force`. Capture is
  routed by `--from-sandbox` (`args::target_name` read `--sandbox`: fixed), the rest by the checkpoint group
  (`restore_steps` removes the computer record first, so the computer cannot name it), and `snapshot list` without a
  group is MicroSandbox's answer with the container checkpoints added. The engine's checkpoint records, retention
  (`reclaim` skips the layer merge for a container: layers are collected when nothing names them), delete, fork and
  quiet-fork code run unchanged. `inspect` reports a network policy (the engine records and compares it for a fork).
- **A "memory snapshots" capability** (`Capability::MemorySnapshots`, false for a container, in the list the app reads).
  `capture_with_kind` and `restore_steps` ask it: a running container is captured with scope disk (no `--full`, no
  pause, no cache drop) and a Restore of a running one captures its disks, stops it (`stop --force`) and replaces it.
  `--full` and `--cow-mem` are refused by the seam as a backstop.
- **Layers** (`containers/layers.rs`): one store, `containers/layers/<id>`; a computer's record names its sealed layers
  (oldest first) and its writable layer by id. A stopped computer's checkpoint seals the writable layer (adds the id to the
  chain, opens a new empty one: no directory owned by a sub-id moves, since layers are made in the store and
  never renamed across folders). A fork is a new record over the checkpoint's chain with a writable layer of its own and
  its own references. Layers nothing names (no record, no checkpoint) are removed by a scan (grace of 10 minutes; a
  record that cannot be read stops the scan). A chain over 16 sealed layers has its oldest 8 folded into one when the
  computer starts (`silo-ctr merge`: whiteouts and opaque directories survive). The overlay options are one page of
  mount data (about 100 bytes a layer), so the cap keeps the kernel's `lowerdir` limits (500 layers, and `lowerdir+=` on
  6.8 and later, which the runner's kernel has) out of play; a start refuses a chain whose option string passes 3400 bytes.
- **Deviation from the design: a running container's checkpoint is a copy.** An overlay mount cannot change its upper
  directory, and the container's file descriptors and working directories keep pointing at the old one, so there is no way to
  seal a live upper and "open a new empty upper" under a running container without restarting its processes. A checkpoint
  of a running container is therefore a copy of the writable layer (`cp -a --reflink=auto` in the container's user
  namespace, so owners and attributes are kept), made while the container's cgroup is frozen (`cgroup.freeze`, waited
  until `cgroup.events` says frozen 1), then thawed. The computer keeps its layer. The cost is proportional to the size of
  the writable layer on a file system without reflinks (the fork point of a fresh dev computer is small), and nothing for a
  stopped one.
- **Engine restart.** A VM computer outlives the engine (a crash or `kill`) and the next engine takes it back. A container's
  gateway was the engine's child, so it was stopped by `reconcile`. Now the holder makes its TAP persistent (kept by the
  network namespace) and the next engine runs `silo-ctr tap`, which joins the holder's namespaces, opens the device again
  and sends it over a socket as the holder does at start; a new gateway serves it, the ports are published again and the
  disk watcher resumes. The GitHub profile is not cached by a new engine, so it is applied by the next refresh. A
  container that cannot be taken back is stopped as before.
- **Disk size (revised after review, 9 Nov 2026).** No rootless mechanism bounds a folder: a project quota is set with
  the administrator's rights on a file system mounted for it, and a loop file needs `mount`, which a user namespace
  cannot do for a block file system. The first CT4 build measured the writable layer and remounted the container's root
  read-only past `root_disk_mib` (`silo-ctr remount`, `quota.rs`, a watcher thread per computer). Review found it does
  not hold: the remount fails while a process has a file open for writing, nothing brings it back after an engine
  restart, a checkpoint resets it, and a directory the engine cannot read is not counted. It is removed (the shim's
  `remount`, `quota.rs`, `watch_disk` and the per-computer watcher state). A container computer now shares the host's disk
  and its size is **reported, not a bound** (threat model, CLI guide, engine guide, changeset). The host is guarded instead
  (`containers/host_disk.rs`): below a margin of free space on the engine's file system (`transfer_area::free_space`; 5 GiB,
  `SILO_CONTAINER_DISK_MARGIN_MIB` in `silod`'s environment, since a Linux host has no settings file; the reason for the value
  is in the module) a container start, checkpoint and fork are refused with a plain message, and one thread of the engine
  (every 30 s, every 60 s while low) stops, with the ordinary graceful stop, the running container computers whose writable
  layer is over their stated size, largest first, leaving the reason in `state/host-disk-notice`, which `inspect` reports as
  `notice` and a start removes. The layer is measured only while the host is low, by `silo-ctr userns -- du -sB1` (so
  directories the engine cannot read count), bounded to 5 minutes. A computer within its size is never stopped, and a stop
  frees nothing; the guard keeps nothing per computer between looks, so a quick stop and start has no watcher state to
  inherit (Opus 4).
- **Review fixes (9 Nov 2026).** (Opus 5) A running container's checkpoint no longer holds the layer-store lock for
  the copy: the copy goes into a new layer id first (safe from collection for the 10-minute grace), the lock is taken to
  register; the processes are frozen for the copy only and at most 30 s (the copy runs in its own process group and is
  killed at the bound; the computer is thawed and the checkpoint fails with a plain message, the half copy removed).
  `flatten` checks `flatten_count` before anything and merges before taking the lock. (Opus 6) A restore retried after an
  engine crash stops a running container only when this run made the capture (journal phase `capturing`); otherwise it
  refuses as for VMs ("resumed after its recovery checkpoint"). (Opus 7) A user's pause is a `paused` file in the state
  folder, separate from the checkpoint's freeze: a checkpoint of a paused container leaves it paused (the freeze guard
  thaws only when there is no file), `inspect` and `list` say Paused, `resume` removes the file, `adopt` thaws only
  without it, and a stop or start removes it; a lock keeps a resume from thawing the processes in the middle of a copy.
  (Opus 8) The quiet step for a container runs after the start (the engine's confirm step), before the fork is reported
  started, so the VM promise that nothing else starts first does not hold; this is documented in the threat model and
  the checkpoints guide, and editing the fork's layer before the start was left out. (Fable) Documented: the `/.msb`
  share is outside any size; dropped layers go at the next collection pass (engine start, a removed checkpoint or
  computer; older than 10 minutes), not at once; `silo-ctr`'s `join_holder` trusts `outer.pid` and relies on the
  engine's `is_holder` check.
- **Quiet forks.** The existing quiet record applies as for a VM, through the engine's confirm step before the fork is
  reported started. A container's script is the VM script with its host name and `/etc/hosts` steps allowed to fail
  (`containers::quiet_script`; the shared script is embedded in the MicroSandbox patch and stays identical).
- **Tests.** Chain, seal, fork layout, flatten, collection and checkpoint store (`layers.rs`), the snapshot and restore
  argument shapes and routing, the fork record, the host guard (margin, refusal message, selection largest first,
  nothing measured with room), the bounded runner, a retried restore that must not stop a running container,
  `silo-ctr merge` and `tap` argument shapes and merge semantics on Linux, the capability in `capture_with_kind` and `restore_steps`, the capability list and
  the app's copy. Live: `linux-host-e2e.sh` with `SILO_E2E_CONTAINER=1` (below).
- **Live check** (KVM runner): `linux-host-e2e.sh` with `SILO_E2E_CONTAINER=1`, run 42561463802 (x86_64, Ubuntu 24.04). Checkpoint of the running container
  (scope `disk`) 0.6 s. `silo fork -n 4` from that checkpoint, four forks created and started: **6.8 s** (the starts, about 5 s each, run
  in parallel). Each fork saw the checkpoint and none saw the parent's later writes, the parent's cron job or another fork's files. `diff` in
  a fork listed `/home/silo/only-1`; `fold` kept a file into the parent and `undo` removed it again (the parent's earlier state stayed). The
  engine unit was killed with SIGKILL: the container was running again with its file and a working public network about 12 s later. A
  container whose recorded size was set to 400 MiB failed a write with "Read-only file system" within a few chunks of 64 MiB
  (this step is replaced by the host guard check below; the remount it checked is gone). The
  first runs failed on the test script (the diff scan does not look in `/workspace`; a plain fork starts stopped; a container's `--disk`
  is only its workspace, so its limit is root disk plus workspace, 81 GiB, and the test shrinks the recorded size).
- **Live check after the review fixes** (KVM runner, code head `681bf0c6b`, Linux host run 42566219752, attempts 3 and 4, both
  green; attempt 1 failed on the VM image download before any container step, and attempt 2 passed the container steps and failed on the same download in the two-device step): the disk-limit step is
  replaced by the host guard. A container whose recorded size was 400 MiB wrote 600 MiB (nothing stopped it while the host
  had room); a `fallocate`d file on the engine's file system left 2.5 GiB free (margin 5 GiB). Within the guard's first
  look (about 37 s) the oversized computer was stopped; `silo start` of it was refused with "This device's disk is nearly
  full (2.5 GiB free; Silo keeps at least 5.0 GiB free), so Silo did not start a container computer. Free some space on
  this device, then try again."; a checkpoint of the other container, which was within its size, was refused with the
  same message and that container kept running; after the file was removed the oversized computer started again.

### 9 Nov 2026: phase CT5 built (the desktop and computer use for container computers)

Branch `claude/containers-ct5`. A container computer has the desktop and computer use through the same API, CLI, MCP and
app as a Linux VM, and the "not yet" list is down to export (CT6) and memory checkpoints (never). Design as decided:
[CONTAINERS-PLAN](CONTAINERS-PLAN.md).

- **Capability.** `Capability::Desktop` is on for the container kind (`containers/capability.rs`, `container-unavailable.json`
  now `["export", "memorySnapshots"]`). Three refusals that existed only for containers are gone: the create-time
  "no desktop yet", the update-time "a container's desktop may not change", and the new-computer rule that kept a
  container from taking its image's desktop (`desktop::default_new_computer_desktops`). A new container is built in and
  starts its desktop like a VM; the app shows "Add Linux desktop" for a container without one.
- **Start without systemd** (`containers/desktop.rs`, called from `containers::start` after the gateway certificate). A VM's init
  runs the image's `silo-desktop-boot`. A container's PID 1 is the shim with no capability, and the desktop service
  switches to the `silo` account with `runuser`, which needs setuid/setgid, so the engine runs the same hook after every start
  as one root command (the capabilities Silo's root commands already get), on a background thread: it waits up to 150 s for
  `/var/lib/silo/working-account.json` (a first start creates the account after the container is up), then runs the hook
  (which does nothing until the desktop's install is recorded as finished); an image with no hook starts nothing. The start does
  not wait for it and a failure is logged, not returned; a hook that fails leaves the marker that makes the desktop's status say
  `failed`. The service's own
  supervisors then keep Xvfb, Xfce and Selkies up, as in a VM. No new container branch outside `engine/src/containers/`:
  the only edits outside are removals of the three refusals above, a guard in `setup-desktop.sh` and the app.
- **No VM-only installer path.** The desktop setup in a container reaches `setup-desktop.sh` through the same engine code; with
  `SILO_RUNTIME=container` (set on every container exec) it exits with a message instead of falling back to `apt-get` when
  the image's desktop is not complete. The branch that provisions the image's preinstalled desktop touches neither apt nor
  the network and runs as for a VM.
- **Chrome with `--no-sandbox`.** `silo-computer-use.py` (pushed and applied after every boot, and when the desktop is turned
  on) writes, in a container only, launchers for `google-chrome`, `google-chrome-stable`, `chromium` and `chromium-browser`
  in `/usr/local/bin` and `google-chrome.desktop` and `chromium.desktop` in `/usr/local/share/applications` that run the launcher, which execs the first installed browser
  (`/opt/google/chrome/google-chrome`, Debian's Chromium paths, whichever name was called) with `--no-sandbox`; a file of that
  name that the helper did not write is left alone. No seccomp exception for Chrome. The app's computer page shows a note when a container has a
  desktop; the threat model, the desktop guide and the computer-use guide say it plainly.
- **Host disk guard (CT4 re-check).** (1) A layer the guard cannot measure (the measurement timed out, or `du` itself failed or
  printed nothing readable) counts as over its size and goes first; layers are measured by up to four workers at once, each bounded to five
  minutes, so one slow container does not hold up the others. (2) The threat model now says the guard's stop can land during a
  checkpoint copy (the frozen scope is killed after the 15 s stop wait; the copy runs outside the container and still
  saves), can act on a measurement minutes old, that sizes adding up to more than the disk can still fill it, and that a
  checkpoint freezes for up to 10 s of waiting plus 30 s of copying.
- **Recipes do not make containers, but a container can be created with its desktop off.** A recipe has no runtime, so a
  recipe's `desktop: false` never makes a container. `computers.create` takes `configuration.desktop =
  {startWithComputer: false}` for a container as for a VM (the app's form sends it); `silo create --no-desktop`
  now sends the same request for both kinds, and the live check creates a container that way.
- **Found on the way** (the KVM runner; four of these failed the first live runs):
  - A root command in a container has no `CAP_SYS_PTRACE`, so it cannot read another account's `/proc/<pid>/exe`. The desktop
    service's identity check of a process it just started (Xvfb) failed, and every container creation failed with "Desktop
    session failed to start". The service now names such a process by its `comm` (`process_exe`), the same way at launch and later.
    The guest's error also carries the end of the service log now (a creation sees only the error), and the engine logs the reason
    a desktop setup failed.
  - **Landlock keeps one exec from reading another's processes.** Each exec runs in its own Landlock domain, and Landlock refuses
    `ptrace` access checks (which `/proc/<pid>/environ` uses) between sibling domains, even for the same account. Cua Driver's
    launcher could not read the desktop session's environment from `xfce4-session` (`X11 setup failed: Authorization required`).
    In a container the session now writes `DISPLAY`, `DBUS_SESSION_BUS_ADDRESS`, `XDG_RUNTIME_DIR` and `XAUTHORITY` to
    `/run/silo-desktop/user/session.env` (mode 0600, the working account's) when it starts, and the launcher and
    `silo-computer-use` read that file when they cannot read the process. (The review fixes below make this the one path for both kinds.)
  - The first start creates the working account after the container is up, so the boot command waits for it.
  - The Linux runner has Google Chrome installed on the host; a guest test for the missing-browser message must not rely on the
    machine having none.
  - The viewer's streamer listens on the container's loopback, and a published port reaches the guest's own address, so the
    URL-proxy check hands a published port to it with a small forwarder in the container; the viewer itself reaches the loopback
    port over the computer's SSH server, as the app does.
- **Tests.** (The review fixes below add the cases for the hook that waits for the install, the failed status, the fixed
  environment of engine commands, `/run` as a tmpfs, `--no-desktop`, the session file reader and its removal, the Chromium entry,
  the streamer password and the unmeasurable layer.) Engine: the capability list and its app copy, the desktop defaults for a container (built in, and
  autostart-off kept), the update validator, the boot command and script, the host guard (unmeasurable layer, ordering, parallel
  measurement). Guest: the Chrome launchers (written for a container only, never over a foreign file, run the browser with the
  flag and its arguments, say so when none is installed). Frontend: the capability model, the overview and detail pages (desktop
  action offered; the browser warning shown for a container only). Live: `linux-host-e2e.sh` with `SILO_E2E_CONTAINER=1`
  (a fourth container with the desktop on, below).
- **Live check** (KVM runner, x86_64, Ubuntu 24.04; Linux host run 42591492385, code head `66888e0c`): a container created with the
  desktop on started it with the computer (service `running`; Xvfb, Xfce and Selkies up). The viewer port answered 401 over an
  `ssh -L` to the container's loopback, through the URL proxy and on its loopback port. Cua Driver (the MCP launcher, as an agent
  starts it) listed the desktop's windows (`Desktop`, `xfce4-panel`) and saved a 1440x900 screenshot (49 KB), uploaded as the
  `container-desktop-screenshots` artifact. Google Chrome (installed from dl.google.com) failed with its own sandbox (exit
  non-zero), started through the launcher, loaded https://example.com through the gateway, and opened a window on the desktop (listed
  by Cua Driver with a second screenshot, 190 KB) whose process has `--no-sandbox`. `silo fork --gui` returned a running fork with
  computer use ready; `silo desktop off` stopped the desktop, `fork --gui` then said "has no desktop; turn it on with `silo desktop on`",
  a restart and a plain fork of it started none, and `silo desktop on` brought it back. `desktop: false` at creation was first
  covered only through `silo desktop off`; the review fixes below add a container created with `--no-desktop`.

#### Review fixes (a code review and a threat-model review of the first build)

- **Desktop off at creation.** The first build said it could not be tested because a recipe cannot make a container; that was
  wrong, since `computers.create` takes `configuration.desktop = {startWithComputer: false}` for a container (the app's form
  sends it). `silo create --no-desktop` (both kinds, not with `--macos`) sends the same request, and the live check creates a
  container that way, starts it and asserts that no desktop, process or service answers.
- **One session-environment path.** The desktop session of a VM now writes `/run/silo-desktop/user/session.env` too;
  `xfce_command` has no `SILO_RUNTIME` branch, the `/proc/<pid>/environ` scans and the `runuser ... cat environ` fallback are
  gone, and the launcher and `silo-computer-use` read the file (root also checks that it is a small regular file of the working
  account, opened without following a link). The desktop stopping removes it. A VM whose session was started by the earlier
  service has no file until its desktop restarts.
- **Disk guard.** Only a measurement that timed out, or a `du` that ran and printed nothing readable, is unmeasurable. The
  measurement is a small script that prints `du`'s exit status, so a failure of `silo-ctr userns` or `newuidmap` (no status) is
  logged and says nothing about the layer, like `CouldNotStart`. An unmeasurable layer is stopped as soon as its own measurement
  ends, not after the whole pass.
- **Boot race.** The hook `setup-desktop.sh` writes does nothing until `install-stage` says `installed`.
- **A failed boot is visible.** The engine's boot command runs the hook with a 120 s limit and, when it fails, leaves
  `/run/silo-desktop/failed`; the desktop status reports `failed` when the session is stopped and that marker exists (the next
  start, a stop or a new boot removes it).
- **The computer's environment and engine commands.** `Exec.fixed_environment` makes `exec_command` use only the engine's
  defaults; the boot command and the gateway certificate update set it. Owner-run `exec` is unchanged.
- **`process_exe`** falls back to `comm` only when `SILO_RUNTIME=container`; in a VM an unreadable `exe` is not recognised, as before.
- **Browsers.** `chromium.desktop` is replaced like `google-chrome.desktop`, and both entries run the launcher, which adds the
  flag once.
- **`/run`** is a tmpfs in the container spec, as a VM's; the cookie and session file do not travel into a checkpoint or a fork.
- **Docs and warning.** The threat model, the app's note, and the desktop and computer-use guides say that a compromised browser
  can act with the computer's GitHub access and secrets and use its in-guest `silo` client, that any Chromium-based program
  started with `--no-sandbox` is exposed the same way, that a launcher runs the first installed browser whatever name was called
  and stays after `desktop off`. The line saying the viewer's credentials stay in root's folder was false (Selkies took the
  password as an argument, readable by every process); Selkies 2.0.0 reads `SELKIES_BASIC_AUTH_PASSWORD`, so the service passes
  it in the streamer's environment and the threat model says who can read that.
- **Live check after the review fixes** (KVM runner, x86_64, code head `8f1342d7d`; Linux host run 42613588062, attempts 1 and 2
  both green; CI run 42613588066 green). The container with the desktop on also shows `/run` as a tmpfs, `session.env` as
  `600:silo`, no `basic-auth-password` in the streamer's arguments, and no `session.env` after `silo desktop off`; the container
  created with `--no-desktop` reports `desktop: false`, starts with no Xvfb, Xfce or Selkies, nothing answering on port 6901, and the
  desktop service `stopped`. Earlier runs on the way failed for the checks' own reasons, not the product's: the helper that runs
  root commands lacked `SILO_RUNTIME=container` (the process-name fallback is now container-only, so status said `failed`); the
  process probes matched their own shell; and a screenshot of a busier Chrome page was larger than one command's output limit, so
  it is copied out in pieces now.


#### Post-merge review: container checkpoints, disk and desktop (CT4 and CT5, 10 Nov 2026)

Review of `engine/src/containers/` (checkpoints, layers, the host disk guard, restart, the desktop's start) and the
guest desktop scripts as they are on `main`. The guest scripts are shared with virtual machines, and two of the
fixes are for those.

- **Fixed: computer use stopped working on virtual machines made before the session file.** The launcher and
  `silo-computer-use` are pushed to every computer after each boot, but a computer keeps the desktop service it was
  made with, and the service of Silo 0.13 or earlier writes no `session.env`. CT5's review made the file the only
  source, so on those machines `doctor` raised `session-unavailable` after every boot and the launcher fell back to
  a bus address that does not exist. Without the file both read the working account's `xfce4-session` process
  again, as they did before the file. Tests: `SessionEnvironment` and `Launcher` in `test_computer_use.py`.
- **Fixed: everything started in the desktop had umask 077.** The command that writes the session file set the
  umask and then ran `startxfce4` in the same shell, in a virtual machine and in a container. The file is written
  in a subshell. Test: `test_the_session_keeps_the_umask_it_was_started_with` (failed before).
- **Fixed: collection could remove a layer that was being made.** The copy of a running computer's layer and the
  fold of a long chain are made outside the store's lock, in a layer nothing names yet, protected only by the
  10-minute grace, which read the folder's modification time. `cp -a` and `silo-ctr merge` give the new folder its
  source's time (the fold after each layer it folds), so the layer looked old while it was still being filled. A
  collection in that window (another checkpoint or computer removed) deleted files under the fold, which then
  replaced the computer's oldest layers with what was left, or removed the copy a checkpoint was about to name. Such
  a layer is now reserved until its record or checkpoint is saved (`layers::reserve`). Test:
  `a_layer_being_made_is_not_collected_however_old_its_files_look`.
- **Fixed: a folder that could not be read counted as empty.** `layers::referenced` listed the computers and
  checkpoints with errors dropped, so a `computers` or `snapshots` folder that could not be opened (no descriptor
  left, an I/O error) named nothing and every layer older than the grace was collected. The scan and `Store::list`
  now fail on anything but a missing folder, and a failed scan collects nothing. Test:
  `a_folder_of_records_or_checkpoints_that_cannot_be_read_stops_the_scan` (failed before).
- **Fixed: a running computer looked as if it were starting while its checkpoint was made.** The checkpoint took
  the start mark for the whole freeze and copy (up to 40 s), and the status reads that mark first. A command run in
  that time was refused with "Start NAME first.", and a secret added or removed was written to the record without
  reaching the running gateway, which kept the old set until the next start. A checkpoint now holds the computer
  against a start without marking it as starting (`StartMark::hold`). Tests:
  `a_held_computer_cannot_be_started_and_is_not_starting`, `a_running_computer_held_for_a_checkpoint_is_still_running`.
- **Not changed, for the owner:** stopping a computer its user paused waits the 15 s for a `TERM` its frozen
  processes cannot take, then kills them; thawing first is the fix, and it has to say what happens to a checkpoint
  copy in flight. Smaller: a fold that fails keeps the computer from starting; the "layers do not fit one mount"
  message says to delete checkpoints, which does not shorten a computer's own chain; one lock serialises the freeze
  of every computer, so a long copy delays another computer's pause, resume or checkpoint.

### 9 Nov 2026: phase CT6 built (export, import and Connections for container computers)

Branch `claude/containers-ct6` ([#104](https://github.com/amontlabs/silo/pull/104)). A container computer is exported to a
`.silo-backup` file, imported under a new name on the same device or another, and exported and imported across Connections, with the
same CLI, API and app as a VM. Design: [CONTAINERS-PLAN](CONTAINERS-PLAN.md).

- **Payload.** A checkpoint of a container is a chain of layers, so the file holds the chain: a header (the checkpoint's name and its
  record without secrets, GitHub access, environment and layer ids) and one zstd frame per layer (`containers/archive.rs`). Each layer is
  a standard tar (PAX) stream, written by `silo-ctr pack` and made by `silo-ctr unpack` with the `tar` crate the engine already uses,
  both run in the container's user namespace (`pack.rs` in the shim). Owners, modes, times, hard and symbolic links, FIFOs, whiteouts
  (character device 0:0) and extended attributes (`SCHILY.xattr.*`, so `user.overlay.opaque` and file capabilities travel) are tar's
  own; tar has no socket type, so **sockets are left out** (nothing listens on one after an import, and the desktop service treats an X
  lock whose socket is missing as stale after a fresh boot); any other device stops the export. Nothing is merged or copied first, so an
  export needs no disk beyond the file. **What makes the unpack safe is where it runs, not the format:** `pack` and `unpack` change their
  root to the layer, so a path or a link in the stream can only name the layer's own files (the reason a plain `tar` extracted as the
  container's root is not used: it can be led through a symbolic link it created earlier). On top of that `unpack` refuses any path with
  `..`, an empty name or an absolute one, and the overlay's attributes other than `user.overlay.opaque`. A first design had a record
  format of Silo's own; two reviews found that it only added a format nobody else reads, and tar covers the same fields.
  **Memory and disk:** a folder's owner and attributes are applied as it is made and only its mode and time wait for the end of the folder
  (the stream lists a folder before its contents, so what waits is the chain of open folders), and the process runs with an address-space
  limit (1 GiB, set by `pack` and `unpack` themselves; checked with a 3 GiB extended header, which ends in "out of memory"). The disk is
  measured, not modelled: `unpack` reads `statvfs` of the new layer before every chunk of a file and every 256 entries and stops under
  the host-disk margin of free bytes or of free inodes (the margin over 16 KiB per inode, ext4's default ratio), so block size, inodes,
  attributes and link targets are counted by the file system. There is no entry limit any more; the inode check replaces it, and `du` is
  no longer used on import.
- **Format 6.** A file with a container row is written as format 6 (4 and 5 as before), so an older Silo refuses it as newer and never
  hands it to MicroSandbox. Rows are marked by the `silo.runtime=container` label in the recorded settings, which must agree with
  `runtime: container` in the computer's settings. The row leaves the image out and names the config digest it needs; an export accepts any digest unpacked on this device
  (a computer made on an older pinned image still exports), and an import accepts the pinned one or one kept unpacked, and otherwise says
  "this computer was made on an older guest image that this device does not have" (the guest image is delivered with Silo; there is no
  registry pull to fall back on). The header is compared with the manifest's settings (cpus, memory, image reference, root disk and workspace, each required)
  before anything is loaded, which is what the descriptor comparison is for a VM.
- **One routing seam.** `snapshot save GROUP:NAME FILE` and `snapshot load FILE --group G --runtime container` are answered by
  `containers::route`/`run` like the other checkpoint commands; `snapshot head` now answers the group's newest checkpoint. The backup
  service's runner (`SystemMsbRunner`) routes through `containers::route`: a container command runs as `silod --container-runtime ...`
  (a child, so it can be timed out and cancelled as MicroSandbox's can), and a list is merged with the container rows. `backup.rs`
  branches only where a payload is made or read (the capture reference, the lean-image row, the extraction, the load and its descriptor
  check), and a captured checkpoint is named `group:name` when the runtime reports no folder for it. `Capability::Export` is gone; the
  list has `memorySnapshots` left, and the app shows Export for a container.
- **Found by the live checks** (each failed a KVM run; none by a unit test):
  - A container's `inspect` answer lacked the computer's name and `kind: managed` in its root disk, which MicroSandbox's export checks
    require; its network section is the gateway's, not the profile (the export records the deny-all profile, as for a VM, and an import
    of a container asks for the gateway's one policy, as a fork does: `containers::imported_network_policy`).
  - The desktop's stale-display check needs the X lock and its socket together; the first design carried sockets for that, and tar cannot,
    so the service now treats a lock without its socket as stale after a fresh boot.
  - The controller of a two-device import has no container computer, so `snapshot list` was MicroSandbox's alone and the import saw no
    group; the list is merged whenever the container checkpoint folder exists.
  - **A home folder with a default ACL** (the CI runner's accounts have one) gave the image's unpacked folders an entry that umoci could
    not restore ("invalid argument"). Silo's folders for container files now have their default ACL removed (`layout::strip_default_acl`).
  - **An engine started from another account's session** carried that account's `XDG_RUNTIME_DIR`, and `systemd-run --user` could not
    reach its own manager ("Failed to connect to bus: Permission denied"). Commands for the user manager now get this account's runtime
    folder when the environment's is not its own.
  - A container's sockets live under its name: with `e2e-hosts-remote-ctr-imported` and `/home/silo-controller/...` the holder's socket
    path passed 107 bytes ("path must be shorter than SUN_LEN"). The live check uses short names; the limit is not changed here.
- **CT5 review notes.** The e2e viewer checks assert 401 without the password and 200 with it (over `ssh -L`, the URL proxy, the loopback
  port and inside the computer). The disk guard test no longer depends on the clock: the slow measurement ends only after the
  unmeasured layer was stopped. The desktop service removes `/run/silo-desktop/failed` when the streamer comes up, so a boot hook that
  gave up waiting does not leave it behind. The comments at `containers/mod.rs` (the boot command is bounded by `BOOT_TIMEOUT`) and
  `desktop-service.py` (what a previous boot left in `/run` is stale; `/run` is a tmpfs in both kinds) are corrected.
- **Review fixes** (two reviewers and a code review of #104). An import that is interrupted or fails on any layer removes the layers
  already made (the guard exists before the first layer), and the checkpoint record says `filling` until the last layer is unpacked, so a
  half-filled layer is not intact, is listed incomplete and is removed by crash recovery like any import group. A checkpoint is taken as a
  VM's or a container's by its computer's row (`is_container_row`; the export journal records it too), never by a missing
  `artifact_path`: for a VM that is an error, so the descriptor comparison cannot be skipped. A host that is not Linux is told
  "Container computers run only on a Linux host." and nothing else. The layer store and the folder holding it are set to `0700` each time
  a layer is made (`lifecycle::seal_store`): a layer's file may be setuid to the engine account (its id is kept in the namespace), so no
  other account may reach it.
- **Tests.** Shim: `pack` 15 (round trip of files, links, hard links, long names, modes, times, whiteouts, opaque folders and attributes,
  sockets left out, the overlay's other attributes refused, the disk margins, hostile paths, devices and a second root, damaged and padded
  streams). Engine: `containers::archive` (header checks, the match with the settings including root disk and workspace and a missing one,
  the refusal on a device that cannot run containers), `containers::layers` (a filling checkpoint), `containers::lifecycle` (the store
  modes), `backup::` (a container exports by `group:name` without the image in format 6; a VM still in 4; the settings a real
  container reports pass the export checks; import through the container runtime and the refusal where none runs; data larger than the
  manifest promises; kind disagreement; format 5 cannot hold a container; the image rules), `route` for an imported checkpoint with no
  container computer, the user-manager environment. Desktop service: the orphaned lock. Frontend: capabilities and the overview page.
  Linux unit tests also ran on the Mac in a container (shim and engine).
- **Live check** (KVM runner, x86_64, Ubuntu 24.04, `linux-host.yml`, code head `24555ef63`): run 42654535736, **attempt 1 and attempt 2
  both green** (CI run 42654535640 green). `linux-host-e2e.sh`: a container with a file, a link, a nested file and a Claude Code login is
  exported (`silo export` without `--yes` and a terminal refuses and names the login; with `--yes` it writes 1.6 MiB), imported as a
  second computer (listed as `container`), started, and its file, link, nested file and login read back; changing the copy leaves the
  source alone; an import under an existing name and a half file are refused and add no computer; after both are dropped no
  `silo-import-*` checkpoint is left. `hosts-e2e.sh` (two accounts): a container made on the host from the controller is listed, run
  in, switched off and on as to its desktop, exported to the controller (the login is named; `--yes` writes 1.6 MiB; neither transfer
  area keeps a file), imported on the controller and onto the host with `--device`, and both copies started and read. Failed runs on the
  way, each for a product reason above: 42625816364, 42628340339, 42631011903, 42635178431, 42639046657, 42642591613, 42647204135,
  42651247266. No run needed a rerun for a GitHub 504.
- **Live check after the review fixes** (tar layers, measured disk, memory limit, image rules): the first run on `67681dc5f`
  (42673505880) failed at the import: overlayfs had written its own `user.overlay.uuid` on the exported layer, and the new unpack
  refuses overlay attributes other than the opaque mark. `pack` now leaves overlayfs's own records behind (it writes them again on the
  next mount; a test covers it). The same push made a VM's export journal carry `"container": false`, which an engine test of the
  journal's shape caught; the flag is now written only for a container. A re-check found the disk look could let a stream write up
  to 256 entries of 64 MiB attributes between two looks on XFS or btrfs; the unpack now also looks whenever names, link targets and
  attributes written since the last look add up to 16 MiB.
- **Live check of the merged head** (`c84c5c114`, the last commit of #104): `linux-host.yml` run 42677509409,
  attempt 1 and attempt 2 both green; CI run 42677509116 green. (The squash commit's message names run
  42677509416, which does not exist; 42677509409 is the run.)
- **Not done.** The viewer and an `ssh` session over Connections for a container are not driven by the two-account run (exec and the
  desktop switch are); they use the same routed methods and `runtime::command_for`. A mid-write abort of `pack` is a kill of the child
  (its layers stay on disk until the grace period collects them). An import whose image this device does not have cannot
  pull it (there is no registry fetch for the guest image). The socket-path length is not raised. A lock without its socket is cleared
  only after a fresh boot (the boot id), so a container re-imported on the same host boot keeps it until the host restarts.

#### Post-merge review: container sharing and docs (CT6, 10 Nov 2026)

Review of the container export and import as they are on `main` after #104 and #106 (`pack.rs` in the shim,
`containers/archive.rs`, the container parts of `backup.rs` and of the backup controller, the prerequisites
refusal), and of the docs of the whole build against the code. Fixes are in the same pull request.

- **Fixed: an imported computer could lose the guest image it needs.** An import is accepted on the pinned image
  or on an older one kept unpacked, and the imported computer has a checkpoint before it has a record (its first
  start makes the record). `prune_storage` kept only the images a record names, so once the last computer with a
  record on the older image was deleted, the next engine start (or the next unpack of the pinned image) removed
  it and the imported computer could never start ("made from an image that this Silo no longer carries"). Pruning
  now also keeps every image a checkpoint names (`layers::images_named`) and removes nothing when a checkpoint
  cannot be read. Test: `an_imported_checkpoint_names_its_image_before_its_computer_has_a_record`. The pinned
  image on a device where no container computer has needed it yet is accepted for the import without being
  unpacked, so a Silo update that pins another image before the computer's first start leaves it without one.
  Unpacking it at import was tried and reverted: it needs the image download, which an account that only imports
  (the two-account live check's controller) does not have, so the import failed (run 42725019568). The threat
  model says so instead. Images and layers are found by one scan of the records (`layers::records`), so a
  computers folder that cannot be read keeps the images as it keeps the layers (the pruning took such a folder
  for empty; the unreadable-folder test of #108 now checks images too and failed before).
- **Fixed: a container import's unpack always had one hour.** A VM's import scans its payload and sizes the load's
  timeout from it; a container's layers are not scanned, so its `snapshot load` got the base hour whatever the
  computer's size, where its export's `snapshot save` gets the hour plus the declared storage at 16 MiB/s. The
  load now gets the same as the save. Test: `a_container_import_gets_time_for_the_storage_its_computer_declares`
  (failed before: 3600 s against 12560 s).
- **Fixed: a full disk during an export was reported as the helper's broken pipe.** When the export file could not
  be written, `silo-ctr pack` ended with a broken pipe and its status was read first ("Packing the computer's
  files failed: ... Broken pipe"). The write failure is now the reason given. Test:
  `a_full_disk_is_named_instead_of_the_helpers_broken_pipe`.
- **Fixed: a refused `snapshot save` removed the file it refused to replace.** The payload file is made with
  `create_new`, and any failure removed whatever was at that path, also when the failure was that the file
  existed. Not reachable from an export (its payload path is in a new private folder), only from the runtime
  command itself. A failed save now removes only a file it made. Test:
  `a_failed_export_removes_the_file_it_made_and_no_other`.
- **Docs corrected.** The threat model said a socket file "comes back as an empty socket file" (sockets are left
  out) and that an import refuses any image but the pinned one (it also accepts one kept unpacked); the command
  line guide said every imported computer is on a deny-all network policy, which is not so for a container (it has
  the one policy its gateway has, public addresses only, from its first start). The Resume above, [PLAN.md](PLAN.md)
  and the status column of [CONTAINERS-PLAN.md](CONTAINERS-PLAN.md) still listed merged phases as open or as "this
  branch"; the container plan said nothing is copied for a checkpoint, which holds for a stopped computer only.
  The CT6 entry had no live check of the merged head; the run is added from GitHub's record. Relative links and
  heading anchors of the build's docs were checked with a script (none broken). The docs index ([README](../README.md)) did not
  list this build's plan, log or the container plan; it has a row for them now.
- **Checked, no change:** `unpack` and `pack` (the changed root, path and link checks, attribute rules, the
  memory limit, the disk looks, what follows the end of a stream); the header checks and the comparison with the
  manifest; labels in a header (those the engine reads are set again by the restore, and the quiet-fork labels
  only act inside the computer); format 6 rules and the kind agreement; the import guard and the removal of a
  half-made import group; the journal flag for a container capture; routing of the snapshot commands and the
  merged list. The export and the engine do not share the in-process store lock (the export's commands run in a
  `silod --container-runtime` child), which is safe because exports and imports hold the engine's operation gate
  and collection leaves a layer younger than ten minutes alone.
- **Left for the owner:**
  1. An imported container computer has the public-addresses network from its first start; an imported VM starts
     on deny-all. It is documented, and it follows from the gateway having one policy, but it is a difference in
     behaviour between the kinds on an untrusted input.
  2. `pack` writes every attribute a file has except overlayfs's own records, so on a host with SELinux it would
     write `security.selinux`, which `unpack` cannot set inside a user namespace on a host without SELinux
     (checked on the Mac in a Linux container: `setfattr -n security.selinux` in `unshare -U -r` is refused with
     "Operation not permitted" where a `user.` attribute is set). No container computer was run on an SELinux host,
     so whether such hosts are in scope, and whether the label should be left out as overlayfs's records are, is
     open.
  3. The sections after the CT6 entry are not in date order (R1, F4, F3, R2); they were left where they are
     because other open pull requests add to them.


## Post-merge review

### Forks and sharing a computer as a file, VM computers (F2 #88, F3 #86, F4 #93; 9 Nov 2026)

Read on `main` at `69742419`: the fork point and the desktop setting, `backup.rs`, `backup_controller.rs`,
`changes/credentials.rs`, `transfer_area.rs`, `export_transfer.rs`, the `exports.*` handlers, the bridge's
`exports.file` stream, `cli/src/share.rs` and the app's export dialog. The review found the problems below; the
fixes are in the same pull request (#105).

- **Fixed: an export could hold memory.** The rule is that no memory is carried into a fork or an export. Forks
  kept it (a fork point is disks only and every fork starts from disks). Exports did not, in three cases, and no
  import ever used the memory (`import_pending_restore_with_environment` records `state: "disk"`; a macOS import
  starts from the disk). All three now carry disks only:
  - **A macOS computer exported while it runs.** `create_archive` called `save_running` and added
    `state.vzvmsave`. It now takes the capture a fork point takes: the guest writes its disk out (`flush_disk`),
    then the machine is paused while the disks are cloned (`pause_for_copy`). No memory is saved and no space is
    reserved for it. An export whose flush fails stops before the pause, as a fork does.
  - **A macOS checkpoint that saved memory.** Its disks are cloned and the saved memory stays in the
    checkpoint's folder. The file names the checkpoint with the kind `disk`.
  - **A Linux VM checkpoint taken while the computer ran: refused.** MicroSandbox 0.7.6 writes such a checkpoint
    whole: `snapshot export` adds `checkpoints/<id>/` with the memory, execution and device state (its
    `archive.rs` at tag `v0.7.6`), and it has no option to leave them out (nor has `v0.7.14`). A fork avoids the
    memory only by starting a new computer from the checkpoint (`restore --disk-only`); that leaves no disk-only
    capture to write, so giving an export the same source would mean a hidden temporary computer for every such
    export. `create_backup_with_token` instead reads the descriptor of every capture before the runtime verifies
    or writes it and goes on only when it names a disk-only capture (scope `file` or `disk`, state `file`).
    Anything else, a missing or unknown scope included, counts as memory and stops the export: "This checkpoint
    was saved while <name> was running, so it holds the computer's memory, and an export never carries memory.
    Fork the checkpoint and export the fork, or export <name> itself." `apply_captured_layout` lost the branch
    that read such a checkpoint's CPUs and memory.

  The reader is unchanged: a file an older Silo wrote with memory still imports, from its disks.
- **Tests.** macOS (`macos_computers.rs`, the capture step `capture_archive_sources` with a machine that fails
  the test if memory is asked for): `an_export_of_a_running_computer_holds_its_disks_and_no_memory`,
  `an_export_of_a_memory_checkpoint_holds_its_disks_and_no_memory`. The capture step is tested instead of the
  written file because the writer wants a disk of the computer's full size. Linux (`backup.rs`):
  `a_checkpoint_that_holds_memory_is_never_written_to_an_export` (refused before `snapshot verify` or
  `snapshot save`, no file left) and `a_running_computer_is_captured_without_its_memory`. Live, on the KVM
  runner: `linux-host-e2e.sh` saves a checkpoint while the computer runs, expects its export to be refused with
  no file, then opens the export of the running computer and the export of a fork of that checkpoint and fails
  on anything under `checkpoints/` or a descriptor that is not a disk capture.
- **Not live-checked: macOS.** There is no macOS computer on the Dev engine; the macOS change is covered by the
  unit tests above only.
- **Fixed: two copies from another device to the same `-o` path shared one `<path>.part`**, so the first to
  finish could publish the second's unfinished file (`export_transfer::create_part` removed the other's as a
  leftover). Each copy now writes `<path>.<random>.part` and nothing that exists is opened or removed; publishing
  is still the rename that never replaces. A copy that crashes leaves its own `.part`, which nothing removes.
- **Docs corrected.** `docs/SiloUI-CLI.md` and `docs/THREAT-MODEL.md` say that no export holds memory, for every
  kind of computer, and what happens to a Linux VM checkpoint that does. The threat model also says which
  folders the login check lists (the `silo diff` walk in `changes/collect.sh`; `/workspace` is not one of them).
  `backup.rs`: the comment on `FORMAT_VERSION` described format 5 while the constant is the newest readable
  format (6). The Resume section and `PLAN.md` listed F4 as an open pull request.
- **Removed** `app/SiloUI/src-tauri/edit14.py`, an editing script committed by mistake in `0b601f2cd`; nothing
  used it.
- **Seen, not changed (low; from reading the code):** a transfer-area export leaves its "Export complete" result
  and a system notice on the owning device, naming a file the controller then discards. `Area::open_for_read`
  keeps its used marker when the file is past the cap.
### Container imports, the runner's checks and the leftovers (10 Nov 2026)
Follows the container sharing review above and R2's left-for-owner list; branch `fix/forks-ci-post-merge-review`.
- **Fixed: imported containers stay on a deny-all network like imported VMs** until a secret is assigned, not
  with the gateway's own policy from its first start. The import records the deny-all policy, the first start
  hands it to the gateway, a fork keeps it, and an export drops it from the portable template (the header
  refuses it, like the other settings the importing Silo decides). Assigning a secret to the computer (`silo
  secret put`, GitHub access among them) unblocks the network at its next start, together with the secret. The
  command line guide's sharing section and the threat model's container sections say so. Tests:
  `an_imported_container_starts_from_the_deny_all_policy_like_a_virtual_machine`,
  `a_secret_unblocks_a_blocked_computer_and_a_refusal_does_not`,
  `a_header_that_could_smuggle_something_is_refused`.
- **Fixed: SELinux labels left out of export and import.** `silo-ctr pack` writes every attribute a file has
  except overlayfs's own records and the SELinux label: the label names types of the exporting host's own
  policy, which `unpack` cannot set inside a user namespace on a host without SELinux, and the importing host
  labels the files it makes itself. Test: `an_export_leaves_out_the_hosts_selinux_label_and_overlayfs_records`.
- **Fixed: the runner re-checks privacy per job (still delivering job events when the check fails)** (R2's item
  1). Every poll that brings jobs runs the private-repository check again before a job is acquired; a public
  repository, or one the check cannot read, is not served (the poll fails and no new jobs are acquired, but
  started and completed job events from the poll are still delivered so finished jobs' computers are cleaned
  up). Tests: `TestAJobIsNotTakenWhenTheRepositoryIsNoLongerPrivate`,
  `TestEventsDeliveredWhenPerJobPrivacyCheckFails`.
- **Fixed: a second Ctrl-C ends a runner shutdown** (R2's item 2). After the first interrupt the shutdown waits
  for the work in flight (deregistrations, stops, drops); another interrupt ends the whole shutdown sequence at
  once (wrapping both listener wait and fleet shutdown) and leaves the work in flight behind — each step
  records what it did, so the next start picks the computers up. Tests:
  `TestASecondInterruptEndsASlowShutdownAtOnce`,
  `TestShutdownSequenceEndsAtOnceOnSecondInterrupt`.
- **Fixed: a paused container stops cleanly.** A paused computer's processes are frozen and cannot run to
  answer TERM, so its stop waited out the grace period and ended in SIGKILL. Stop ends the pause first — under
  the same lock a checkpoint's copy holds — and then signals. Tests:
  `a_stop_ends_a_pause_before_it_signals_the_processes`,
  `a_stop_waits_for_a_checkpoint_before_it_ends_a_pause`.
- **Fixed: a second checkpoint of a container computer says one is in progress**, not that the computer is
  starting. Test:
  `a_second_checkpoint_says_one_is_in_progress_not_that_the_computer_is_starting`.
- **The Status sections are in date order** (R1 and F3 between F2 and C1, F4 and R2 between CT3b and CT4); no
  wording in them changed.
