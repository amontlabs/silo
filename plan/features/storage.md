## Proposed, not confirmed by the owner

### For the owner to settle

- Are Silo's disk images left out of Time Machine by default, which keeps main out of the Mac's backups until backup-host is built?
  - Recommendation: Yes, with a line in the Storage tab that says so, because a disk image that changes all day makes every backup huge.

### For whoever builds it

- Detail: Disk use is shown per computer, per fork and per checkpoint, in the app and the command line.
- Detail: The disk view names images and build folders, stale build output inside a fork included.
- Detail: Stale build output inside a fork can be reclaimed on its own.
- Detail: On a Mac, Silo's disk images, fork data and caches are excluded from Spotlight by default.

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
