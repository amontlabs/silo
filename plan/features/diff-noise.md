## From the old code

The old code had it built, about 800 lines, and it needs reworking.

Lessons:
- Never measure a baseline on the used fork: its capture holds the task's own changes.

Where it lived, at the tag legacy-2026-11-16:
- `docs/SiloUI-CHANGES.md#buckets`
- `docs/SiloUI-RECIPES.md#fork-baselines`
- `docs/SiloUI-CHANGES.md#L8`
- `app/SiloUI/src-tauri/engine/src/changes/service.rs`

Calls: `silo diff --baseline`, `changes.baseline`.
