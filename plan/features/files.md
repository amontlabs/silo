## From the old code

The old code had it built and driven end to end, about 5,800 lines, and its approach can be reused.

Lessons:
- The stock exec command starts stopped computers; a --no-start patch was needed so browsing never boots one.
- A hint is never authoritative: clients re-read.
- Remote names reach sftp only through commands that do not expand wildcards; local sources are staged behind a fixed symlink name.
- msb copy starts stopped computers and writes root-owned files, so system sftp was chosen.
- Never verified against a real guest or a remote computer.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/guest.rs`
- `app/SiloUI/src-tauri/silod/src/methods/guest.rs`
- `app/SiloUI/src-tauri/engine/src/guest/files.rs`
- `app/SiloUI/src/features/application/components/computer-file-tree.tsx`
- `app/SiloUI/src/features/application/model/directory-store.ts`
- `app/SiloUI/src-tauri/src/commands/files.rs`
- `docs/SiloUI-FILES.md#L175`
- `app/SiloUI/src-tauri/guest/list-directory.py`
- `app/SiloUI/src-tauri/engine/src/guest/watch.rs`
- `app/SiloUI/src-tauri/engine/src/guest/hints.rs`
- `app/SiloUI/api/src/methods/files.rs`
- `app/SiloUI/src-tauri/engine/src/files.rs`

Calls: `guest.files.list`, `guest.files.read`, `guest.files.write`, `guest.files.remove`, `guest.files.watch`, `files.list`, `files.upload`, `files.download`.
