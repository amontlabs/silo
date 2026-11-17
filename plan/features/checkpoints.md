## From the old code

The old code had it built and driven end to end, about 8,000 lines, and it needs reworking.

Lessons:
- An interrupted checkpoint could block the next start until its dependencies were released (R10).
- A computer's agent checkpointing itself always makes a manual checkpoint so retention never removes the owner's automatic and step checkpoints.
- Restore first saves a recovery checkpoint and leaves the computer stopped.
- Forking an earlier checkpoint of itself asks the owner: it can hold access since taken away.
- Snapshot data and its graph belong to MicroSandbox; Silo keeps only names, intent and references, and removes without --force so upstream guards still apply.
- A snapshot something still needs (a fork not started yet, an import, a restore's recovery point) must be pinned by Silo records.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/src-tauri/silod/src/methods/checkpoints.rs`
- `app/SiloUI/api/src/methods/checkpoints.rs`
- `docs/direction/evidence/F.txt`
- `app/SiloUI/src-tauri/silod/src/methods/checkpoints/tests.rs`
- `app/SiloUI/cli/src/ops.rs`
- `docs/SiloUI-CHECKPOINTS-PLAN.md`
- `docs/SiloUI-POLICY.md#method-by-method`
- `docs/product/JOURNEYS.md#L57`
- `app/SiloUI/src-tauri/engine/src/runtime/checkpoints.rs`
- `app/SiloUI/src-tauri/engine/src/checkpoints/mod.rs`
- `app/SiloUI/src-tauri/engine/src/runtime/checkpoints/native.rs`
- `app/SiloUI/src/features/application/components/checkpoint-panel.tsx`

Calls: `checkpoints.create`, `checkpoints.restore`, `checkpoints.delete`, `checkpoints.abandonRestore`, `checkpoints.usage`, `silo checkpoint`, `silo checkpoints`, `silo restore`.
