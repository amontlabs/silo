## Notes

- Old design: a manifest taken inside the computer at fork time and on the fork (allow-listed files, a shadow git of config folders, package lists, each repository's HEAD); the host compares them.
- Groups: Code, Packages, Settings and files, Other changed, Not kept, Ignored (a count). JSON and Markdown output; a `.siloignore` file.
- Also wanted: which package a change came from, changed git-ignored files such as `.env`, "database changed" from a schema hash, credential paths listed under Not kept.
- Command output must stream past the old exec limits (180 s, 768 KiB).
- Target: most real tasks read in 15 lines or fewer.

## From the old code

The old code had it partly built, about 6,400 lines, and it needs reworking.

Lessons:
- Silo's own files (endpoint.json, pulse files) must be ignored or every fork conflicts on them (M C2).
- Silo's own per-computer files (/etc/silo, receipts, machine ids) must be on the ignore list or every fork shows them as changes.
- Item ids are built only from what names the item, so the same id works in diff, conflict and choice.
- Idle macOS forks rewrite many preference and database files; they need a built-in ignore list.
- Databases are shown but not driven live.
- guest.exec is capped at 768 KiB and 180 s, so the collector runs detached in the guest and is read back in chunks.
- A known record with too few fields is an error, because a truncated stream would otherwise read as deleted files.
- The fork-time base is captured at the fork's first start, after the quiet step and before it reports ready.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/changes.rs`
- `app/SiloUI/src-tauri/silod/src/methods/checkpoints/changes.rs`
- `app/SiloUI/src-tauri/engine/src/changes`
- `docs/SiloUI-CHANGES.md`
- `docs/direction/evidence/M.txt`
- `docs/SiloUI-CHANGES.md#buckets`
- `app/SiloUI/cli/src/ops.rs`
- `docs/direction/evidence/T5a.txt`
- `docs/product/JOURNEYS.md#L43`
- `app/SiloUI/src-tauri/engine/src/changes/diff.rs`
- `app/SiloUI/src-tauri/engine/src/changes/ignore.rs`
- `app/SiloUI/src-tauri/engine/src/changes/collect.rs`

Calls: `changes.diff`, `changes.capture`, `changes.baseline`, `silo diff`.
