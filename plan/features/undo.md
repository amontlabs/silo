## Proposed, not confirmed by the owner

### For the owner to settle

- Does undo only put a computer back to before its last merge, or can it also take back one merged change and keep the rest?
  - Recommendation: The whole last merge only: it is exact, while taking back one change is a reverse merge that can conflict.

### For whoever builds it

- Detail: Besides the state before a merge, any conflicted or agent-resolved state during it is kept as a checkpoint the user can return to.

## From the old code

The old code had it built and driven end to end, about 800 lines, and its approach can be reused.

Lessons:
- Undo restores everything since the merge, including unrelated work like agents installed afterwards (V I3b).
- Undo of a replace only reverts the latest one involving either computer.
- A before-merge checkpoint becomes undoable only after its merge succeeded, and is consumed once restored.

Where it lived, at the tag legacy-2026-11-16:
- `docs/direction/evidence/V.txt`
- `app/SiloUI/api/src/methods/keep.rs`
- `app/SiloUI/src-tauri/silod/src/methods/checkpoints/keep.rs`
- `docs/SiloUI-CHANGES.md#merging-a-fork-silo-merge-and-silo-undo`
- `app/SiloUI/cli/src/flows.rs`
- `docs/product/JOURNEYS.md`
- `docs/product/JOURNEYS.md#L47`
- `app/SiloUI/src-tauri/engine/src/changes/keep/journal.rs`
- `app/SiloUI/src-tauri/engine/src/changes/promote/service.rs`

Calls: `changes.undo`, `silo undo`.
