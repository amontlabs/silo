# Silo v1 build log

Spec: `~/Documents/work/funding/PRODUCT.md`. Brief: `~/Documents/work/funding/MASTER_PROMPT.md`.
Validation report: `~/Documents/work/funding/research/silo-directions/95_validation.md` (prototypes at `6e0fa58c:bench/prototypes/`).
Tracker: Linear project "Silo v1: git for agent computers" (P-AMO-8).

## Resume

- **Phase:** 0 (AMO-88), in progress.
- **Open slices:** 0a status reads during jobs; 0b checkpoint prune and retention; 0c app assessment.
- **Clean up:** nothing yet.
- **Next action:** review and merge phase 0 slices, build the Dev engine, run the phase 0 VM check.
- **macOS slots:** one foreign VM (PID 60843, running since 2 Nov) holds one of the two slots.

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

### Phase 0c: Silo app assessment (4 Nov, read-only)

- **Builds and runs on the current engine:** likely, not yet proven here. The app is a thin client of `silod` over `engine.sock`; `engine_boundary_tests.rs` forbids in-process engine calls and method-name literals, so engine API changes break the app at compile time (macOS CI `cargo nextest --workspace` compiles `src-tauri`). The protocol is a single number (`api/src/transport.rs:60`, client offers exactly 7 in `client/src/session.rs:36-42`), so app and engine ship together. CI never packages or launches the app; the baseline `desktop:build:debug` is the evidence for that.
- **How a screen calls the engine:** no router; `application-app.tsx` switches pages. React store → `src/desktop/commands.ts` → `#[tauri::command]` in `src-tauri/src/commands/` → `engine_client` → typed `silo_api::requests::*` → `run_job`. Each new command needs a `build.rs` manifest entry, handler registration, a `FakeEngine` forwarding test and a permissions test. `checkpoint-panel.tsx` and `computer-detail-page.tsx` are reusable.
- **Phase 7 cost:** about 7 forwarding commands, 3 new pages or components (review/keep/throw away with a new diff view is the largest; steps reuses the checkpoint panel; pending changes needs a per-device fan-out with "unavailable" rows), 3 to 4 fixtures and their tests, plus contract JSON updates.
- **Risk to the backend:** medium-low if phase 7 stays deferred and additive. Shared churn points: `api/src/methods/*`, `schema.rs`, `src/contracts/silo.ts`, `src/test/contracts/*.json`.
- **Recommendation:** bring the app back in phase 7; touch it before then only to keep CI green. Re-check at phase 7: the debug bundle builds and the Dev app connects at the then-current protocol; frontend checks and the command manifest pass; the review, step and tree methods are stable with typed constructors; the state shape is frozen for the phase; wording follows `docs/SiloUI-VOCABULARY.md` and `UI-PATTERNS.md`.

## Decisions

- **4 Nov, checkpoint disk (phase 0b), advised by Fable:** a live probe on the pinned runtime (`docs/v1/evidence/phase-0/raw/checkpoint-disk-probe.txt`) showed a computer's checkpoints form one linear msb chain, the engine refuses every checkpoint delete while the computer exists, disk comes back only when the computer is deleted, and images do not leak. The pinned msb 0.7.6 has no top-level `msb prune` (the validation's prune figures came from upstream 0.7.13). Two reviews (gpt-6.1-sol, Opus; Argon failed to run) found that retention by deletion frees nothing on a linear chain and that `msb image prune` can drop layers a loaded, not yet started snapshot needs. Fable read the msb source: each checkpoint holds its whole layer closure, so parent links are lineage only, and msb ships `msb modify --compact`, which merges old sealed layers into a new base. Decision: no automatic image prune; retention = compact the computer's oldest layers, then remove the pruned Silo-owned checkpoints with `--force`, always keeping the newest. Spec change pending the measurement in the phase 0 VM check.
- **4 Nov, checkpoint disk follow-up, advised by Fable:** two compaction probes (`compact-probe.txt`) showed compaction (running or stopped) costs about 0.8-1.1 GB and removing every older snapshot afterwards returns about 0. Cause in the pinned msb: each capture's layer closure is staged in the sandbox's `runtime/checkpoints/<id>/` and never removed after publication, so those hard links hold the data until the sandbox is deleted. Separately, `publish_layer_closure` copies every qcow2 layer on each capture, so the first capture after a restore cost 1-5 GB. Decision: carry two Silo msb patches (release the staging dir after publication plus a crash sweep; link instead of copy when the backing name already matches), re-probe, then implement retention as compact + forced removal. Fallback if the copy patch fails: accept growth in v1, show checkpoint disk per computer, and record the spec change.
- **4 Nov, merging to main:** main has a GitHub ruleset that refuses direct pushes (pull request plus 7 required status checks). Each phase lands as a squash-merged PR from `claude/project-thread-e72ch6` once CI is green and reviews pass, per the project rules. This replaces "commit directly to main".

## Spec changes

## Status
