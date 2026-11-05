# Silo v1 build log

Spec: `~/Documents/work/funding/PRODUCT.md`. Brief: `~/Documents/work/funding/MASTER_PROMPT.md`.
Validation report: `~/Documents/work/funding/research/silo-directions/95_validation.md` (prototypes at `6e0fa58c:bench/prototypes/`).
Tracker: Linear project "Silo v1: git for agent computers" (P-AMO-8).

## Resume

- **Phase:** 1 checks passed; PR to main pending CI. Phase 0 merged in #60.
- **Open slices:** none.
- **Clean up:** none outstanding. The Dev engine runs from the main checkout's bundle.
- **Blocked on the owner:** the positive `--secret NAME` fork check and a non-trivial "no assignments by default" need the owner to allow the Dev engine's Keychain access once (`credential_access_denied`). Read-only GitHub against a mock needs a GitHub App installation the CLI can't create; covered by unit tests only.
- **Next action:** merge the phase 1 PR, then start phase 2 (change capture + bucketed diff, Keep + Undo).
- **macOS slots:** the foreign VM (PID 60843) is gone; both slots free as of 5 Nov.

## Start

- Start commit: `7121ecc8ed9b7593b9040d21038301f6aab7a836` (main, 4 Nov 2026). The final wording check diffs against it.
- AMO-70 (Cua computer use) is on main: squash-merged as #18 (`6975192f`) and #32 (`d2ac5839`); branch `claude/project-thread-q2ne7l` has no commits after #32. Its Linear status stays In Progress because its remaining scope is phase 5 of this build.
- No other Agent-labelled issue outside this build's list was In Progress.
- Creation fix: `0dae8805` is not an ancestor of main, but its change landed on main in equivalent form in #11 (`74bd4f0a`, `silod/src/startup.rs` `install_creation_inputs`). Not cherry-picked.
- Production engine (`org.silo.preview.engine`, PID 1238, Fjord's embedded engine) is running and is never touched. The Dev engine agent (`org.silo.dev.engine`) is registered but not running.

## Phase plans

### Phase 0 (AMO-88)

| Slice | What | Files | Review |
|---|---|---|---|
| 0a | `computers.list` and status reads survive running jobs; client retry | `engine/src/runtime/mod.rs` (`read_application_snapshot`), `client/` | reviewer |
| 0b | `msb prune` after drop and checkpoint removal; chained snapshots removed leaf to root; never prune a base a retained checkpoint needs; retention count for pre-keep checkpoints | `engine/src/runtime/checkpoints.rs`, `engine/src/checkpoints/` | reviewer + Argon |
| 0c | Read-only Silo app assessment (builds and runs on the current engine; cost of phase 7 screens) | none | none |

VM check: create and fork a Linux computer through `engine.sock` while status polls keep succeeding; disk returns after drop and checkpoint removal; the newest retained checkpoint still restores.

### Phase 1 (AMO-74, AMO-77, AMO-89; AMO-63 landed in phase 0)

| Slice | What | Files | Review |
|---|---|---|---|
| 1a | `silo` CLI + MCP server (`silo mcp`, stdio), engine discovery on `~/.silo-dev`/`~/.silo`, job waiting, `.silo/recipe.yaml` and `.silo/silo.yaml` loaders; phase 1 commands: `list`, `status`, `create`, `start`, `stop`, `fork`, `drop`, `exec`, `checkpoint` | new crate `app/SiloUI/cli` | reviewer |
| 1b | Quiet forks, Linux: API contract (`secrets`, `quiet` on `checkpoints.fork`), cold boot only, no secret/GitHub assignment copy unless named, new machine-id and hostname, scheduled jobs off and `scrub:`/`quiet:` applied before guest boot | `api/src/methods/checkpoints.rs`, `engine/src/runtime/checkpoints.rs`, `engine/src/checkpoints/`, guest scripts | reviewer + Argon |
| 1c | macOS fork fast path: one boot, no developer-tools reinstall, no extra shutdowns; `New Password: Permission denied` | `engine/src/macos_computers/` | reviewer + Argon |
| 1d | Quiet forks, macOS (after 1c): automatic checkpoint when none named, `--secret` opt-in, scheduled jobs (LaunchAgents/cron) off, `scrub:`/`quiet:` | `engine/src/macos_computers/checkpoints.rs` | reviewer + Argon |

VM check (Linux and macOS): a macOS fork boots once; a source with a boot-persistent scheduled job (incl. `@reboot`), a `quiet:` poller and a `scrub:` file; its fork shows no polls or job runs, a new machine-id and hostname, no assignments by default, `scrub:` paths gone; read-only GitHub against a mock GitHub API.

### Phase 0c: Silo app assessment (4 Nov, read-only)

- **Builds and runs on the current engine:** likely, not yet proven here. The app is a thin client of `silod` over `engine.sock`; `engine_boundary_tests.rs` forbids in-process engine calls and method-name literals, so engine API changes break the app at compile time (macOS CI `cargo nextest --workspace` compiles `src-tauri`). The protocol is a single number (`api/src/transport.rs:60`, client offers exactly 7 in `client/src/session.rs:36-42`), so app and engine ship together. CI never packages or launches the app; the baseline `desktop:build:debug` is the evidence for that.
- **How a screen calls the engine:** no router; `application-app.tsx` switches pages. React store → `src/desktop/commands.ts` → `#[tauri::command]` in `src-tauri/src/commands/` → `engine_client` → typed `silo_api::requests::*` → `run_job`. Each new command needs a `build.rs` manifest entry, handler registration, a `FakeEngine` forwarding test and a permissions test. `checkpoint-panel.tsx` and `computer-detail-page.tsx` are reusable.
- **Phase 7 cost:** about 7 forwarding commands, 3 new pages or components (review/keep/throw away with a new diff view is the largest; steps reuses the checkpoint panel; pending changes needs a per-device fan-out with "unavailable" rows), 3 to 4 fixtures and their tests, plus contract JSON updates.
- **Risk to the backend:** medium-low if phase 7 stays deferred and additive. Shared churn points: `api/src/methods/*`, `schema.rs`, `src/contracts/silo.ts`, `src/test/contracts/*.json`.
- **Recommendation:** bring the app back in phase 7; touch it before then only to keep CI green. Re-check at phase 7: the debug bundle builds and the Dev app connects at the then-current protocol; frontend checks and the command manifest pass; the review, step and tree methods are stable with typed constructors; the state shape is frozen for the phase; wording follows `docs/SiloUI-VOCABULARY.md` and `UI-PATTERNS.md`.

## Decisions

- **4 Nov, checkpoint disk (phase 0b), advised by Fable:** a live probe on the pinned runtime (`docs/v1/evidence/phase-0/raw/checkpoint-disk-probe.txt`) showed a computer's checkpoints form one linear msb chain, the engine refuses every checkpoint delete while the computer exists, disk comes back only when the computer is deleted, and images do not leak. The pinned msb 0.7.6 has no top-level `msb prune` (the validation's prune figures came from upstream 0.7.13). Two reviews (gpt-6.1-sol, Opus; Argon failed to run) found that retention by deletion frees nothing on a linear chain and that `msb image prune` can drop layers a loaded, not yet started snapshot needs. Fable read the msb source: each checkpoint holds its whole layer closure, so parent links are lineage only, and msb ships `msb modify --compact`, which merges old sealed layers into a new base. Decision: no automatic image prune; retention = compact the computer's oldest layers, then remove the pruned Silo-owned checkpoints with `--force`, always keeping the newest. Spec change pending the measurement in the phase 0 VM check.
- **4 Nov, checkpoint disk follow-up, advised by Fable:** two compaction probes (`compact-probe.txt`) showed compaction (running or stopped) costs about 0.8-1.1 GB and removing every older snapshot afterwards returns about 0. Cause in the pinned msb: each capture's layer closure is staged in the sandbox's `runtime/checkpoints/<id>/` and never removed after publication, so those hard links hold the data until the sandbox is deleted. Separately, `publish_layer_closure` copies every qcow2 layer on each capture, so the first capture after a restore cost 1-5 GB. Decision: carry two Silo msb patches (release the staging dir after publication plus a crash sweep; link instead of copy when the backing name already matches), re-probe, then implement retention as compact + forced removal. Fallback if the copy patch fails: accept growth in v1, show checkpoint disk per computer, and record the spec change.
- **5 Nov, quiet forks scope:** "scheduled jobs off" means scheduled jobs (cron, at, launchd jobs with intervals, calendars or watch paths, periodic scripts); services that keep running (a Postgres or other dev-stack service) stay, since the fork should carry the dev stack. Anything else an agent wants stopped goes in the recipe's `quiet:` list. The macOS "Fork point" is its own checkpoint kind, disk scope, kept while a fork needs it as the fork-time base for Keep.
- **5 Nov, URL per fork:** `http://<port>.<computer>.silo.localhost:<proxy port>` on one loopback listener instead of binding each guest port on the host, because binding `127.0.0.1:3000` blocks the user's own dev server on `0.0.0.0:3000` (gpt-6.1-sol review). The spec's `:3000` example becomes the guest port in the name.
- **4 Nov, merging to main:** main has a GitHub ruleset that refuses direct pushes (pull request plus 7 required status checks). Each phase lands as a squash-merged PR from its own branch (`claude/silo-v1-phase-N`; phase 0 used `claude/project-thread-e72ch6`) once CI is green and reviews pass, per the project rules. This replaces "commit directly to main".

## Spec changes

## Status

### 5 Nov 2026: phase 2 passed

- **Checks (engine + CLI built from this branch, MicroSandbox 0.7.6 with 22 patches; Ubuntu 24.04.4 and macOS 26.6.2 guests):** Linux 39/39 (`linux-check.txt`): three live `claude -p` tasks through the `silo` MCP server gave 9, 9 and 10 line diffs with the package, migration (Code + Database changed), `.env` (gitignored) and settings in the right buckets; a plain keep ran the migration, restarted the web server (new PID), tests pass on main; a divergent keep was refused with main byte-identical; an uncovered data change was refused and pointed to Make this main; Undo returned main to the pre-keep fingerprint. Undo exactness re-run 3/3 identical including the Postgres data digest, main left running without a script start (`linux-undo-rerun.txt`). macOS 25/25 (`macos-check.txt`): a live task edited a LaunchAgent and installed jq; 14-line diff; keep installed jq, applied the agent change; undo back. Restart attested from outside the engine with a code-only change: agent PID 436 → 4993, undo resumed main from memory (`macos-restart-rerun.txt`). Evidence: `docs/v1/evidence/phase-2/`, scripts `docs/v1/checks/phase2_*`.
- **Evidence review (Opus):** found the first runs' undo flaky on Postgres sequences, undo leaving main stopped, and the macOS restart attested only by the engine; fixed (undo restores the checkpoint's memory and restarts main) and re-run.
- **Shipped:** change capture with the blind-spot fixes, `silo diff` with an idle-fork baseline, Keep with three-way checks and `keep:` hooks, Undo; reviews by gpt-6.1-sol and Opus found and fixed issues around root-run git honouring repo config, a pg_dump option injection, half-applied keeps, credentials in commit history, and churn-triggered refusals.
- **Decisions:** see Decisions; macOS churn handled by an ignore list built from observed idle churn plus the idle-fork baseline (Apple preference domains are ignored, so a deliberate `defaults write` to an Apple domain does not show; revisit if it matters).
- **Next:** phase 3 (recipes, `silo up`, Make this main, `-n`, `fork ./dir`, URL check).

### 5 Nov 2026: phase 1 passed (two items blocked on the owner)

- **Checks:** Linux and macOS quiet-fork checks through the `silo` CLI only (engine and CLI at `726020f2`, MicroSandbox 0.7.6 with 22 patches, Ubuntu 24.04.4 and macOS 26.6.2 guests). Sources had boot-persistent jobs (Linux: XDG autostart starting a poller and cron with a minute job and `@reboot`; macOS: StartInterval and KeepAlive LaunchAgents, a minute crontab and `@reboot`), confirmed running after a source restart. Quiet forks over 130 s: no job or poller lines from the fork and no log growth on either OS; new machine-id/identifier, hostname and host keys (also for a fork of a fork); scrubbed file gone in the fork and kept on the source; quiet record applied; macOS fork booted once (`boots == 1`, one `boot 1: first Start` line), 26 s from start to ready (dev signal only). A Linux control fork without a recipe ran the unnamed poller and no scheduled jobs. One check line reads FAIL by design of the script: the cron daemon itself still starts from an XDG autostart entry in the quiet fork, with its crontab and `@reboot` parked, so no job runs; the spec asks for no job runs, which holds. Evidence: `docs/v1/evidence/phase-1/` (`linux-quiet-check.txt`, `macos-check.txt`, earlier runs beside them). Scripts: `docs/v1/checks/phase1_*_check.sh`. Workspace tests pass; frontend 2812 tests pass.
- **Shipped:** `silo` CLI and MCP server (`app/SiloUI/cli`) with the recipe and settings loaders; quiet forks on Linux (boot hook in the Silo msb patch runs the quiet step before the desktop) and macOS (offline parking during fork setup); cold-boot forks only; no assignments copied unless named, `github` read-only; macOS fork in one boot with offline personalization and network staging (was 4 boots); macOS create no longer fails on the password step; URL per fork (AMO-91, early).
- **Evidence review (Opus; Argon could not run):** found the first runs showed several conditions for the wrong reason (no boot-persistent source jobs, no control). Re-run with persistent jobs and a control; that re-run found two real bugs (XDG autostart entries and KeepAlive LaunchAgents named in `quiet:` still started), both fixed and re-checked.
- **Decisions:** quiet scope and URL format (see Decisions). Reviews by gpt-6.1-sol and Opus.
- **Spec changes:** none. The URL example becomes `http://<port>.<fork>.silo.localhost:<proxy port>`; PRODUCT.md says "e.g.", so no text change is needed yet.
- **Blocked:** the positive `--secret` copy and a non-trivial "no assignments by default" check need your Keychain approval for the Dev engine; read-only GitHub against a mock needs a GitHub App installation (unit-tested). Known issue outside v1 scope: macOS disks stay at 40 GiB because the Recovery partition blocks the container grow (now a note, not a failure).
- **Wording:** clean over added lines. One commit message on the branch (`e3a457f1`) uses a listed word; it does not reach main because PRs are squash-merged with a clean message.
- **Next:** phase 2.

### 4 Nov 2026: phase 0 passed

- **Checks:** phase 0 VM check passed on the Dev engine (engine `a235c34a`, MicroSandbox 0.7.6 with 22 patches): 1033 `computers.list` polls at 250 ms through create, fork, retention and delete with 0 failures; fork disk came back (+5 MiB after delete); before-keep retention at count 2 kept the newest 2 and left +11.1 GB free compared with keeping all 5, the next capture worked; the newest retained checkpoint restored with the right data; nothing left after delete. Evidence: `docs/v1/evidence/phase-0/vm-check.txt`, earlier failed runs beside it, probes in `raw/`. Script: `docs/v1/checks/phase0_check.py`. Workspace tests: 3293 passed, 0 failed (before the last fixes); engine and silod suites pass after them.
- **What changed:** status reads serve the last published inventory during device-wide work and live configuration changes; two Silo MicroSandbox patches (release per-capture staging dirs after publication; delete retired layers after compaction); retention compacts old layers then removes trimmed checkpoints, as its own job on the computer's lane; forks restore disk-only; unreferenced memory cache files are removed; a `before-keep` checkpoint kind with its own retention count. AMO-63 signing and notarization landed early (dry-run mode).
- **Decisions:** checkpoint disk design advised by Fable (see Decisions). Reviews by gpt-6.1-sol, then Opus when Codex and Antigravity quotas ran out; Argon could not run (it asked for file-read permission).
- **Spec changes:** none yet. The Disk row's "msb prune" wording describes upstream 0.7.13; the pinned runtime returns checkpoint disk through retention (compaction + removal) instead. I will fold this into PRODUCT.md with the phase 1 status after Fable confirms the wording.
- **Blocked on a foreign VM:** nothing in phase 0 (no macOS step). PID 60843 still holds one macOS slot.
- **Next:** phase 1 (CLI + MCP, quiet forks, macOS fork fast path).
- **Needed from you:** for AMO-63, the Apple Developer ID certificate and App Store Connect key as secrets (list in `docs/SiloUI-RELEASES.md`). Optional: post the two upstream MicroSandbox PR drafts in `docs/v1/upstream/`.
