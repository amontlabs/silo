## From the old code

The old code had it built, about 1,400 lines, and its approach can be reused.

Lessons:
- Silo never force-resets a repository; a repo conflict can only go to a branch or stay as main.
- Silo never force-resets a repository: a conflicting one can only go to a branch or stay as main.
- Fast-forward must not swap computers, because the agent lives in main.
- Silo's own per-computer files and per-login files conflicted on every merge until left out.
- Undo was flaky on Postgres sequences in v1.

Where it lived, at the tag legacy-2026-11-16:
- `docs/SiloUI-CHANGES.md#conflicts-are-questions`
- `docs/direction/evidence/V.txt`
- `docs/SiloUI-CHANGES.md#L245`
- `app/SiloUI/src-tauri/engine/src/changes/items.rs`
- `app/SiloUI/src-tauri/engine/src/changes/keep/plan.rs`
- `app/SiloUI/src-tauri/engine/src/changes/keep/cron.rs`
- `docs/DIRECTION.md#L112`
- `docs/product/JOURNEYS.md#L45`
- `docs/v1/evidence/phase-2/linux-check.txt`

Calls: `silo merge --choose`, `silo merge --skip`, `changes.keep`, `silo merge`, `silo replace-main`, `silo undo`.
