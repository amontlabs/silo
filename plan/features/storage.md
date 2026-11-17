## From the old code

The old code had it partly built, about 1,300 lines, and its approach can be reused.

Lessons:
- The runtime's discard-by-truncate optimisation shrank disks; a vendored fix keeps disk length while punching holes.
- msb-imago truncated a disk when discard reached its end, shrinking it until mount failed with EINVAL; discard must punch holes and keep length.
- Report reclaimed bytes from host allocation, not what fstrim prints.
- A runtime tail-discard bug shrank disks on reopen; the fix is in Silo's patch.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/storage.rs`
- `docs/direction/evidence/M.txt`
- `docs/SiloUI-STORAGE-RECLAMATION.md`
- `docs/SiloUI-STORAGE-DISCARD-RESEARCH.md`
- `docs/SiloUI-STORAGE-RECLAMATION.md#L126`
- `app/SiloUI/src-tauri/engine/src/runtime/storage.rs`
- `app/SiloUI/src/features/application/pages/workspace-storage-panel.tsx`
- `app/SiloUI/src-tauri/src/commands/runtime_storage.rs`
- `app/SiloUI/docs/silo-help.html`

Calls: `storage.read`, `storage.reclaim`.
