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
