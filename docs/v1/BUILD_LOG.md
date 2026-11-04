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

## Decisions

## Spec changes

## Status
