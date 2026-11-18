## Proposed

- The docs say that a checkpoint with memory resumes the whole computer after a crash, so an agent program needs no resume machinery of its own.
- A fork can be paused from the app, which saves a checkpoint with its memory and stops it, and resumed where it was.
- A pause, of one fork or of all, can first let each agent finish its current step, so the work resumes from a clean point.

## From the old code

The old code had it built, about 800 lines, and it needs reworking.

Lessons:
- MicroSandbox never deletes restored memory files under cache/memory/snapshots; Silo must remove files no checkpoint names.
- A resource edit that invalidates saved memory needs an explicit disk-only decision; never drop memory silently.
- Container computers have no memory in their checkpoints.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/src-tauri/engine/src/runtime/checkpoints.rs`
- `app/SiloUI/src-tauri/engine/src/runtime/memory_cache.rs`
- `docs/SiloUI-CHECKPOINTS-PLAN.md`

Calls: `checkpoints.create`, `checkpoints.restore`.
