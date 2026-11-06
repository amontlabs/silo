# Silo v1 build log

Spec: `~/Documents/work/funding/PRODUCT.md`. Brief: `~/Documents/work/funding/MASTER_PROMPT.md`.
Validation report: `~/Documents/work/funding/research/silo-directions/95_validation.md` (prototypes at `6e0fa58c:bench/prototypes/`).
Tracker: Linear project "Silo v1: git for agent computers" (P-AMO-8).

## Resume

- **Phase:** 7 on branch `claude/silo-v1-phase-7`, PR open. Phases 0-6 merged (#60-#65, #70).
- **Done in phase 7:** app screens for pending changes, review (keep / throw away / open fork) and steps (fork from step, fork tree), driven once in the real app (`docs/v1/evidence/phase-7/app-check.txt`).
- **Owner steps:** approve `docs/v1/launch/` drafts; Keychain repair (phase 4 credentials check; phase 1/3 secret checks); release steps in `docs/v1/release/README.md`; post `docs/v1/upstream/` drafts; drop leftovers `e2e-p3-macsrc`, `e2e-p3-macsrc-before-20261105-2253` and `silo-baseline-8fa22f` (the last needs the Keychain repair).
- **Next action:** merge the phase 7 PR when CI is green; v1 build complete.

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

### Phase 4 (AMO-79, AMO-69, AMO-92, AMO-84, AMO-93, AMO-94; credentials-used line)

| Slice | What | Review |
|---|---|---|
| 4a | Review requests in the engine API: list, show, keep, throw away, open fork, with buckets and the credentials line; CLI and MCP on the same methods | reviewer |
| 4b | Fork from step + fork tree: parent links, per-step checkpoints, step listing, `--from-step N`, `silo tree` + API | reviewer |
| 4c | Credentials-used line: network-crate patch logging successful substitutions (Linux runtime rebuild), macOS proxy log capture, per-computer aggregator and method | reviewer + Argon |
| 4d | Per-agent settings (`agents:`), recipe write-back on keep, `silo fold --pr` (host GitHub connection or runner `GITHUB_TOKEN`) | reviewer |
| 4e | Auto-keep rules (`auto_keep:`, `fold: auto-code`) | reviewer |
| 4f | GitHub Action `silo-action/` (not published), tested with a local runner | reviewer |

All tests use the mock GitHub API, a local bare git remote and a loopback HTTP endpoint with a fake secret. VM check: only the credentials line (Linux and macOS).

### Phase 5 (AMO-85, AMO-86, AMO-87, AMO-70)

| Slice | What | Review |
|---|---|---|
| 5a | In-guest client: guest-to-host endpoint with a scoped credential enforcing the settings file (`fold: ask` agents fork and request, never keep, promote or grant secrets); threat-model doc | reviewer + Argon |
| 5b | Mac mini remote: new `OWNER_METHODS`, one pending-changes list across engines, actions routed to the right machine; tests with a second Dev engine | reviewer |
| 5c | Hosted reviewer: engine/CLI send only the diff to a configurable endpoint, three lenses, advisory banner, forbidden-word filter; service folder tested as a local mock, nothing deployed | reviewer |
| 5d | Native computer use on AMO-70: Cua Linux image built locally if unpublished, host MCP bridge, "fork, start, wait until ready", `auto` approval for unattended forks, diff ignore entries, macOS port with permissions pre-granted | reviewer + Argon |

VM checks: in-guest client from inside a Linux main; computer use on Linux and macOS (drive a fork's GUI from the host).

5a VM check (in-guest client, Dev build, Linux main, a throwaway `e2e-*` computer; see `docs/SiloUI-IN-GUEST-CLIENT.md`):

1. Build a guest `silo` (`cargo build -p silo-cli --release --target aarch64-unknown-linux-musl`) or reuse a Linux build.
2. On the Mac, in a project whose `.silo/silo.yaml` has `agents: { openclaw: { fold: ask, fork_secrets: [] } }`: `silo guest-client install <main> --agent openclaw --binary <silo>`; `silo guest-client status <main>`.
3. Confirm `ls <runtime home>/sandboxes/<main>/runtime/silo-bridge/{req,resp}` exists on the Mac and that inside the guest `ls -ld /.msb/silo-bridge/req` shows the same folder, writable by the agent's user, and `/etc/silo/guest-client.json` is readable by it.
4. Inside the guest: `silo --in-guest list` (only main and its forks), `silo --in-guest fork main --name e2e-in-1`, `silo --in-guest diff e2e-in-1` after changing a file in the fork, `silo --in-guest fold e2e-in-1 --request`; then on the Mac `silo requests` shows it.
5. Refusals, inside: `silo --in-guest fold e2e-in-1`, `--promote`, `undo main`, `drop e2e-in-1`, `secrets list` all print "not available from inside a computer". Hand-write a frame for `changes.keep` and `guest.exec` into `req/` and confirm `permission_denied` answers.
6. In the fork `e2e-in-1`: `/etc/silo/guest-client.json` is gone; copying the parent's file there and calling `silo --in-guest list` is refused.
7. `silo fold e2e-in-1 --promote` on the Mac, then inside the old main: `silo --in-guest list` is refused (token revoked).
8. Clean up: `silo drop` the e2e computers, `silo guest-client revoke <main>`.

### Phase 6 (AMO-71, AMO-80; final run)

| Slice | What |
|---|---|
| 6a | OpenClaw and Hermes recipes with quiet and scrub presets (done; lines marked CONFIRM are checked in the final run) |
| 6b | Final run on the finished Dev build, Linux and macOS, evidence in `docs/v1/evidence/final/`: demo flow (up, fork -n 4, review, keep, undo, throw away), quiet forks, a macOS fork, one OpenClaw recipe, computer use, promote over main; measurements (Linux fork of a running main, 3 cold runs; -n 4 from the warm checkpoint; fork RAM with `footprint`; macOS fork-to-ready) |
| 6c | Release prep under `docs/v1/release/`: workflow dry run, artifacts, notes, the owner's remaining steps |
| 6d | README, 60-s demo script and numbers post drafts from the 6b numbers only (owner approves public text) |

## Decisions
- 2026-11-06, measurement build (Fable): numbers come from a Dev-identity, release-profile bundle (`build_desktop.py --dev-release`) of the same commit, with the same runtime binaries as a release; not the production app, which agents may not drive. The post states machine, runtime versions, N runs with median/min/max, and what stops the clock. One Linux cold fork is also run on the debug bundle as a cross-check.
- 2026-11-06, steps (AMO-69): a step is one command an agent runs on a computer through Silo (CLI exec/run, MCP run). Per-step checkpoints are off by default (`step_checkpoints: true` turns them on; the last `step_checkpoints_keep`, default 20, are kept per computer). `--from-step N` forks from the checkpoint taken after step N.

- **4 Nov, checkpoint disk (phase 0b), advised by Fable:** a live probe on the pinned runtime (`docs/v1/evidence/phase-0/raw/checkpoint-disk-probe.txt`) showed a computer's checkpoints form one linear msb chain, the engine refuses every checkpoint delete while the computer exists, disk comes back only when the computer is deleted, and images do not leak. The pinned msb 0.7.6 has no top-level `msb prune` (the validation's prune figures came from upstream 0.7.13). Two reviews (gpt-6.1-sol, Opus; Argon failed to run) found that retention by deletion frees nothing on a linear chain and that `msb image prune` can drop layers a loaded, not yet started snapshot needs. Fable read the msb source: each checkpoint holds its whole layer closure, so parent links are lineage only, and msb ships `msb modify --compact`, which merges old sealed layers into a new base. Decision: no automatic image prune; retention = compact the computer's oldest layers, then remove the pruned Silo-owned checkpoints with `--force`, always keeping the newest. Spec change pending the measurement in the phase 0 VM check.
- **4 Nov, checkpoint disk follow-up, advised by Fable:** two compaction probes (`compact-probe.txt`) showed compaction (running or stopped) costs about 0.8-1.1 GB and removing every older snapshot afterwards returns about 0. Cause in the pinned msb: each capture's layer closure is staged in the sandbox's `runtime/checkpoints/<id>/` and never removed after publication, so those hard links hold the data until the sandbox is deleted. Separately, `publish_layer_closure` copies every qcow2 layer on each capture, so the first capture after a restore cost 1-5 GB. Decision: carry two Silo msb patches (release the staging dir after publication plus a crash sweep; link instead of copy when the backing name already matches), re-probe, then implement retention as compact + forced removal. Fallback if the copy patch fails: accept growth in v1, show checkpoint disk per computer, and record the spec change.
- **6 Nov, Make this main and the macOS proxy CA, advised by Fable:** the brief lists "(macOS) proxy CA" among what a promote carries; PRODUCT.md does not mention it. Each macOS computer's guest trusts its own proxy CA (Keychain item keyed by computer id, verified at start); only that guest and its own proxy use it, and a fork already replaces an inherited CA at start. Carrying main's CA made the promoted computer fail the start check and would need re-trusting the guest. Decision: each computer keeps its own proxy CA and machine identifier; the promote carries main's name, assignments, SSH alias, ports, lineage, hostname and scheduled jobs. No user-visible loss (Fable checked: host apps never trust these CAs). No PRODUCT.md change needed.
- **6 Nov, warm checkpoint reuse:** a fork reuses `silo up`'s warm checkpoint only when the engine saw no change to main since (a per-computer change generation) and it is at most 60 s old; plain `ssh` edits bypass the engine, which the docs state.
- **5 Nov, quiet forks scope:** "scheduled jobs off" means scheduled jobs (cron, at, launchd jobs with intervals, calendars or watch paths, periodic scripts); services that keep running (a Postgres or other dev-stack service) stay, since the fork should carry the dev stack. Anything else an agent wants stopped goes in the recipe's `quiet:` list. The macOS "Fork point" is its own checkpoint kind, disk scope, kept while a fork needs it as the fork-time base for Keep.
- **5 Nov, URL per fork:** `http://<port>.<computer>.silo.localhost:<proxy port>` on one loopback listener instead of binding each guest port on the host, because binding `127.0.0.1:3000` blocks the user's own dev server on `0.0.0.0:3000` (gpt-6.1-sol review). The spec's `:3000` example becomes the guest port in the name.
- **4 Nov, merging to main:** main has a GitHub ruleset that refuses direct pushes (pull request plus 7 required status checks). Each phase lands as a squash-merged PR from its own branch (`claude/silo-v1-phase-N`; phase 0 used `claude/project-thread-e72ch6`) once CI is green and reviews pass, per the project rules. This replaces "commit directly to main".

## Spec changes

## Status

### 6 Nov 2026: phase 7 passed

- **Assessment re-checked:** the app built, its 2820 frontend tests passed, and the backend already had every method the screens need; the screens only add to the app.
- **Shipped:** Tauri commands for review requests, steps and the fork tree; Pending changes page (all devices, unreachable devices shown); review page (buckets, diff text, credentials line, stale state, keep with a confirmation naming the target, throw away, open fork with copyable URLs); steps panel with fork from a step and a fork tree. Also: requests of deleted forks or targets are closed (`closedReason`).
- **Reviews:** Opus; findings fixed (stale wording, keep confirmation, no undo claim without an undo control, URL opening, error messages, contract test types).
- **Real-app check:** each screen driven once in Silo Dev with LCU against live data: keep carried the fork's file into main (CLI confirms), fork from step 1 created the fork. Evidence: `docs/v1/evidence/phase-7/app-check.txt` (screenshots could not be saved; texts recorded).
- **Tests:** frontend 2854 passed; Rust command and permission tests pass.

### 6 Nov 2026: phase 6 final run passed

- **Final run** (commit `09677470`, Dev-identity release-profile build, M4 Max 64 GiB, macOS 26.5, MicroSandbox 0.7.6 with 24 Silo patches): demo flow (up, fork -n 4, review, keep, undo, throw away), quiet forks, computer use, promote over main and undo, cold and warm measurements, macOS forks: 25 passed. OpenClaw recipe: 9 passed. Measurements (N=3, median, min–max): cold Linux fork of a running main 9.80 s (9.47–12.49); `-n 4` from the warm checkpoint 17.06 s (16.45–19.69); macOS fork to ready 59.19 s (58.97–60.35); RAM per Linux fork about 500 MiB (497.7–502.2 over 5). Evidence and method: `docs/v1/evidence/final/README.md`.
- **Fixed during the run:** quiet entries naming a recipe service; `up` invalidating its own warm checkpoint; the throwaway sweep slowing forks; a `--dev-release` bundle's CLI believing it was production (it now takes its channel from the bundle, and the engine links only a CLI of its own channel); warm reuse after sessions over `ssh main.silo` (MicroSandbox patch reports SSH sessions); quiet names for launchers; SSH listener retry backoff; CLI link errors reported apart from Connections.
- **Release prep:** dry runs pass; changesets plan 0.12.0 → 0.13.0; the `silo` CLI is now built, signed and bundled on the real release path.
- **Drafts for approval:** `docs/v1/launch/` (README, 60-s demo script, numbers post).
- **Next:** phase 7 assessment.

### 6 Nov 2026: phase 5 passed

- **Shipped:** in-guest `silo` client (spool on the `/.msb` share read through directory fds, per-computer token stored as a hash, allow-list scoped to forks the token made, caps, revoke on promote/undo/delete, runtime folder sealed except `silo-bridge`, static guest binary via `app/SiloUI/scripts/build-guest-silo.sh`); Mac mini remote (v1 methods routed over Connections, `--device`, `--all-devices`, one request list across devices, MCP device actions gated by `agents.<name>.devices`); hosted reviewer (`silo review`, three lenses, advisory banner, wording filter, only a reduced diff and a capped, redacted code diff sent; `reviewer-service/` tested offline, not deployed); computer use from the host (one driver session per fork on the Cua driver 0.38.0 contract, `fork --gui`, `auto` approval for forks only, re-applied after keep and promote, ignore entries).
- **Reviews:** every slice by Opus, two or three rounds; second reviews by Argon (until its quota ran out) and gpt-6.1-sol. Main finds fixed: the guest could redirect the spool to host files; fork ownership by name; a remote fanning out past one hop; reviewer URL from a project file receiving the token; driver tool names guessed wrong; root-run chowns following guest symlinks; a guest user reaching `root-disk.json` on the runtime share.
- **VM checks (Dev engine, live):** in-guest client from inside a Linux main, 59/59 (`docs/v1/evidence/phase-5/inguest-check.txt`, earlier runs beside it); computer use 32 passed, 1 blocked (Linux and macOS forks driven from the host: typed into a GUI terminal, file appeared; keep and approval checks), the blocked one needs a computer named main (`computer-use-check.txt`). Live runs found and fixed: non-root bridge access, a msb refusal to checkpoint a running computer after the host removed share files (worked around with a guest cache drop; upstream draft in `docs/v1/upstream/msb-virtiofs-unlinked-checkpoint.md`), promote archive names over 32 characters, keep writing agent config as root, macOS readiness and approval.
- **Tests:** workspace suites pass (engine 3083).
- **Wording:** clean over added lines (remaining matches are git's `safe.directory`, rustc's `link-self-contained`, and the reviewer filter's own word list).
- **Next:** phase 6.

### 6 Nov 2026: phase 4 built; credentials VM check blocked on the owner

- **Shipped (PR #64):** review requests in the engine API (list, show with buckets and the credentials line, keep pinned to the request's digest, throw away limited to the request's own fork, open, markKept), with `silo requests` and MCP tools; optional per-step checkpoints, `silo steps`, `silo fork --from-step N`, `silo tree`/`computers.tree`; the credentials-used line (MicroSandbox patch and macOS gateway write name, host, method and blocked reasons, never values; per-key counters survive log rotation; `computers.credentials`; shown in `silo status`, `silo diff`, `silo requests show`); `agents:` per-agent settings (entries may only narrow `recipes` and `fork_secrets`; `fold`/`auto_keep` overrides apply only when the agent is named by `--agent`/`SILO_AGENT`), `auto_keep:` rules and `fold: auto-code`, recipe write-back after a keep, `silo fold --pr`; `silo-action/` (not published).
- **Reviews:** each slice reviewed by Opus (Codex quota ran out), the credentials slice also by Argon; three rounds on `--pr` (found: a fork could redirect the push and receive the host token; folder auto-keep not pinned; MCP client name widening settings) and two on review requests (ownership of throw away, double keep after a crash). All fixed with tests.
- **Tests:** silo-api, silod, silo-cli and silo-engine pass locally (engine 3046 passed); frontend typecheck, lint and the schema tests; `silo-action/test/run.sh` 18 checks. All GitHub paths use a local mock API and bare remotes.
- **VM check:** BLOCKED on Linux and macOS: `silo secrets set` returns `credential_access_denied` (the rebuilt Dev engine cannot read Keychain items an earlier build made). Computer create/start passed; the rest of the script has not run yet. Evidence: `docs/v1/evidence/phase-4/credentials-check.txt`.
- **Decisions:** steps (see Decisions); `--pr` uses `GITHUB_TOKEN` or `gh auth token` on the host, never the fork's credentials, and builds the push URL on the host.
- **Wording:** clean over added lines.
- **Next:** merge #64 after CI; Keychain repair; re-run the credentials check; phase 5.

### 6 Nov 2026: phase 3 passed (secret carry-over blocked on the owner)

- **Checks (Dev engine + CLI at `9d1eba7f`, MicroSandbox 0.7.6 with 23 patches):** URLs (`urls-check.txt`, 8/8): `silo up` made `main` from the recipe and a warm checkpoint; `silo fork main -n 4` made four forks from it, each serving :3000 at `http://3000.<fork>.silo.localhost:<proxy port>` with its own hostname, all four answering at once; dropped forks' URLs stop answering. Make this main, Linux (`promote-check.txt`, Linux half all pass) and macOS (`promote-check-macos.txt`, 12/12): the fork takes main's name, content, URL, ports, `ssh main.silo` and scheduled jobs, the old main is archived and stopped, the new main keeps main's running state; Undo swaps back exactly. `fork ./dir` (`folder-check.txt`, 11/11): a host repo's commit made in the fork comes back with a backup ref, Undo restores the host branch, dirty-tree and new-commit cases refused. macOS fork 2-6 s and fork-to-ready about 35 s, Linux `fork -n 4` 15 s: dev signals only, not published.
- **Shipped:** recipes and the recipe cache, `silo up` with a warm checkpoint (reused only when main is unchanged and at most 60 s old), `fork -n` with `fork_ram_budget` and the macOS slot limit, `fork -- cmd`, `fork ./dir`, fold requests, `silo ports/publish/unpublish`, `silo ssh-setup`, Make this main on Linux (new `microsandbox-rename-stopped` patch) and macOS, URL per fork.
- **Reviews:** gpt-6.1-sol and Opus on every slice; fixes included host-side git hook execution and overwrite checks for `fork ./dir`, rename journal crash safety, nested lock ordering in promote, and stale warm checkpoints.
- **Decisions:** proxy CA stays per computer; warm checkpoint reuse rule (see Decisions).
- **Blocked on the owner:** each Dev engine rebuild loses Keychain access to items earlier builds made, so secret carry-over can't be checked and three leftover test computers (`silo-baseline-8fa22f`, `e2e-p3-macsrc`, `e2e-p3-macsrc-before-20261105-2253`) can't be dropped by me.
- **Next:** phase 4.

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
